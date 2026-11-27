//! Sandboxed per-extension Lua runtime and [`ExtensionHost`].

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context};
use mlua::{
    Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt, RegistryKey, StdLib, Table, Value,
    VmState,
};
use parking_lot::Mutex;

use crate::manifest::{DiscoveredExtension, ExtensionManifest, Permission};
use crate::types::{
    CommandRegistration, EditorContext, ExtensionInfo, HostRequest, NotifyLevel, PanelContent,
    PanelItem, PanelRegistration, ThemeRegistration,
};

/// Default per-extension Lua heap limit (64 MiB).
pub const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// Default per-invocation Lua instruction limit.
pub const DEFAULT_INSTRUCTION_LIMIT: u64 = 5_000_000;
const HOOK_STEP: u32 = 10_000;

#[derive(Default)]
struct CallbackStore {
    commands: HashMap<String, RegistryKey>,
    panels: HashMap<String, (String, RegistryKey)>,
    events: HashMap<String, Vec<RegistryKey>>,
}

struct LoadedExtension {
    manifest: ExtensionManifest,
    dir: PathBuf,
    lua: Lua,
    callbacks: Arc<Mutex<CallbackStore>>,
    instruction_count: Arc<AtomicU64>,
}

/// Manages discovery, lifecycle, permissions, and execution of sandboxed Lua extensions.
pub struct ExtensionHost {
    extensions_dirs: Vec<PathBuf>,
    granted: HashMap<String, HashSet<Permission>>,
    disabled: HashSet<String>,
    loaded: HashMap<String, LoadedExtension>,
    last_errors: HashMap<String, String>,
    requests: Arc<Mutex<Vec<HostRequest>>>,
    context: Arc<Mutex<EditorContext>>,
    memory_limit: usize,
    instruction_limit: u64,
}

impl ExtensionHost {
    /// Create a new [`ExtensionHost`] scanning `extensions_dirs` with the given per-extension
    /// granted permissions.
    pub fn new(
        extensions_dirs: Vec<PathBuf>,
        granted: HashMap<String, Vec<Permission>>,
    ) -> Self {
        let granted_sets = granted
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect();
        Self {
            extensions_dirs,
            granted: granted_sets,
            disabled: HashSet::new(),
            loaded: HashMap::new(),
            last_errors: HashMap::new(),
            requests: Arc::new(Mutex::new(Vec::new())),
            context: Arc::new(Mutex::new(EditorContext::default())),
            memory_limit: DEFAULT_MEMORY_LIMIT,
            instruction_limit: DEFAULT_INSTRUCTION_LIMIT,
        }
    }

    /// Override the per-extension Lua memory limit in bytes (default: 64 MiB).
    pub fn set_memory_limit(&mut self, bytes: usize) {
        self.memory_limit = bytes;
    }

    /// Override the per-invocation Lua instruction limit (default: 5,000,000).
    pub fn set_instruction_limit(&mut self, instructions: u64) {
        self.instruction_limit = instructions;
    }

    /// Replace the granted permissions for an extension ID.
    pub fn set_granted_permissions(&mut self, id: impl Into<String>, perms: Vec<Permission>) {
        self.granted
            .insert(id.into(), perms.into_iter().collect());
    }

    /// Enable or disable an extension by ID. Disabling also unloads it if currently loaded.
    pub fn set_enabled(&mut self, id: &str, enabled: bool) {
        if enabled {
            self.disabled.remove(id);
        } else {
            self.disabled.insert(id.to_string());
            self.unload(id);
        }
    }

    /// Returns whether an extension is enabled.
    pub fn is_enabled(&self, id: &str) -> bool {
        !self.disabled.contains(id)
    }

    /// Discover all valid extensions across configured directories.
    pub fn discover(&self) -> Vec<DiscoveredExtension> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        for root in &self.extensions_dirs {
            let Ok(entries) = std::fs::read_dir(root) else {
                continue;
            };
            let mut dirs: Vec<PathBuf> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_dir())
                .collect();
            dirs.sort();
            for dir in dirs {
                let manifest_path = dir.join("extension.toml");
                if !manifest_path.is_file() {
                    continue;
                }
                match ExtensionManifest::load(&manifest_path) {
                    Ok(manifest) => {
                        if seen.insert(manifest.id.clone()) {
                            let enabled = self.is_enabled(&manifest.id);
                            found.push(DiscoveredExtension {
                                manifest,
                                dir,
                                enabled,
                            });
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            path = %manifest_path.display(),
                            error = %err,
                            "skipping invalid extension manifest"
                        );
                    }
                }
            }
        }
        found
    }

    /// Return runtime status information for all discovered extensions.
    pub fn list_info(&self) -> Vec<ExtensionInfo> {
        self.discover()
            .into_iter()
            .map(|d| {
                let loaded = self.loaded.contains_key(&d.manifest.id);
                let last_error = self.last_errors.get(&d.manifest.id).cloned();
                ExtensionInfo {
                    manifest: d.manifest,
                    dir: d.dir,
                    enabled: d.enabled,
                    loaded,
                    last_error,
                }
            })
            .collect()
    }

    /// Load all enabled discovered extensions. Errors for individual extensions are recorded
    /// and returned as `(extension_id, error)` pairs without aborting other extensions.
    pub fn load_all(&mut self) -> Vec<(String, anyhow::Error)> {
        let discovered = self.discover();
        let mut errors = Vec::new();
        for ext in discovered {
            if !ext.enabled {
                continue;
            }
            let id = ext.manifest.id.clone();
            if let Err(err) = self.load_from_discovered(ext) {
                self.last_errors.insert(id.clone(), err.to_string());
                errors.push((id, err));
            }
        }
        errors
    }

    /// Load (or reload) a single extension by ID.
    pub fn load(&mut self, id: &str) -> anyhow::Result<()> {
        if !self.is_enabled(id) {
            bail!("extension '{id}' is disabled");
        }
        let ext = self
            .discover()
            .into_iter()
            .find(|d| d.manifest.id == id)
            .ok_or_else(|| anyhow!("extension '{id}' not found"))?;
        match self.load_from_discovered(ext) {
            Ok(()) => {
                self.last_errors.remove(id);
                Ok(())
            }
            Err(err) => {
                self.last_errors.insert(id.to_string(), err.to_string());
                Err(err)
            }
        }
    }

    /// Unload a loaded extension by ID, dropping its isolated Lua VM.
    pub fn unload(&mut self, id: &str) -> bool {
        self.loaded.remove(id).is_some()
    }

    /// Update the current editor/workspace context exposed to extensions.
    pub fn set_context(&mut self, ctx: EditorContext) {
        *self.context.lock() = ctx;
    }

    /// Drain all pending [`HostRequest`]s queued by extensions.
    pub fn drain_requests(&mut self) -> Vec<HostRequest> {
        std::mem::take(&mut *self.requests.lock())
    }

    /// Execute a command registered by any loaded extension.
    pub fn run_command(&mut self, command_id: &str) -> anyhow::Result<()> {
        let ext = self
            .loaded
            .values()
            .find(|ext| ext.callbacks.lock().commands.contains_key(command_id))
            .ok_or_else(|| anyhow!("unknown extension command '{command_id}'"))?;

        ext.instruction_count.store(0, Ordering::Relaxed);
        let ctx_tbl = build_context_table(&ext.lua, &self.context.lock())?;
        let func: Function = {
            let cbs = ext.callbacks.lock();
            let key = cbs
                .commands
                .get(command_id)
                .ok_or_else(|| anyhow!("command '{command_id}' not registered"))?;
            ext.lua.registry_value(key)?
        };
        func.call::<()>(ctx_tbl)
            .map_err(|e| anyhow!("command '{command_id}' failed: {e}"))?;
        Ok(())
    }

    /// Render a panel registered by a loaded extension into a [`PanelContent`].
    pub fn render_panel(&mut self, panel_id: &str) -> anyhow::Result<PanelContent> {
        let ext = self
            .loaded
            .values()
            .find(|ext| ext.callbacks.lock().panels.contains_key(panel_id))
            .ok_or_else(|| anyhow!("unknown extension panel '{panel_id}'"))?;

        ext.instruction_count.store(0, Ordering::Relaxed);
        let (default_title, func): (String, Function) = {
            let cbs = ext.callbacks.lock();
            let (title, key) = cbs
                .panels
                .get(panel_id)
                .ok_or_else(|| anyhow!("panel '{panel_id}' not registered"))?;
            (title.clone(), ext.lua.registry_value(key)?)
        };

        let val: Value = func
            .call(())
            .map_err(|e| anyhow!("panel '{panel_id}' render failed: {e}"))?;
        parse_panel_value(&default_title, val)
    }

    /// Emit a named event (e.g. `"file_opened"`, `"file_saved"`, `"workspace_opened"`)
    /// to all loaded extensions subscribed via `blur.on`.
    pub fn emit(&mut self, event: &str, payload: serde_json::Value) -> anyhow::Result<()> {
        let mut first_err: Option<anyhow::Error> = None;
        for ext in self.loaded.values() {
            let funcs: Vec<Function> = {
                let cbs = ext.callbacks.lock();
                match cbs.events.get(event) {
                    Some(keys) => keys
                        .iter()
                        .filter_map(|k| ext.lua.registry_value::<Function>(k).ok())
                        .collect(),
                    None => continue,
                }
            };
            let lua_payload = ext.lua.to_value(&payload)?;
            for func in funcs {
                ext.instruction_count.store(0, Ordering::Relaxed);
                if let Err(err) = func.call::<()>(lua_payload.clone()) {
                    tracing::warn!(
                        extension = %ext.manifest.id,
                        event = %event,
                        error = %err,
                        "extension event handler failed"
                    );
                    if first_err.is_none() {
                        first_err = Some(anyhow!(
                            "extension '{}' handler for '{}' failed: {err}",
                            ext.manifest.id,
                            event
                        ));
                    }
                }
            }
        }
        if let Some(err) = first_err {
            return Err(err);
        }
        Ok(())
    }

    /// Return the directory of a loaded extension, if loaded.
    pub fn loaded_dir(&self, id: &str) -> Option<&Path> {
        self.loaded.get(id).map(|e| e.dir.as_path())
    }

    fn effective_permissions(&self, manifest: &ExtensionManifest) -> HashSet<Permission> {
        let Some(granted) = self.granted.get(&manifest.id) else {
            return HashSet::new();
        };
        manifest
            .permissions
            .iter()
            .copied()
            .filter(|p| granted.contains(p))
            .collect()
    }

    fn load_from_discovered(&mut self, discovered: DiscoveredExtension) -> anyhow::Result<()> {
        let id = discovered.manifest.id.clone();
        let entry_path = discovered.dir.join(&discovered.manifest.main);
        let source = std::fs::read_to_string(&entry_path).with_context(|| {
            format!(
                "failed to read entry script for extension '{id}' at {}",
                entry_path.display()
            )
        })?;

        // Isolated Lua 5.4 VM with restricted stdlib (no io/os/package/debug).
        let stdlib = StdLib::TABLE
            | StdLib::STRING
            | StdLib::MATH
            | StdLib::UTF8
            | StdLib::COROUTINE;
        let lua = Lua::new_with(stdlib, LuaOptions::default())
            .map_err(|e| anyhow!("failed to create Lua VM for '{id}': {e}"))?;

        // Ensure dangerous globals are absent.
        {
            let globals = lua.globals();
            for forbidden in ["io", "package", "debug", "dofile", "loadfile"] {
                globals.set(forbidden, Value::Nil)?;
            }
            globals.set("os", build_safe_os(&lua)?)?;
        }

        let effective_perms = Arc::new(self.effective_permissions(&discovered.manifest));
        let callbacks = Arc::new(Mutex::new(CallbackStore::default()));
        let blur_table = build_blur_api(
            &lua,
            id.clone(),
            effective_perms,
            Arc::clone(&self.requests),
            Arc::clone(&self.context),
            Arc::clone(&callbacks),
        )?;
        lua.globals().set("blur", blur_table)?;

        // Install instruction-count hook to abort runaway scripts.
        let instruction_count = Arc::new(AtomicU64::new(0));
        let counter_hook = Arc::clone(&instruction_count);
        let max_instructions = self.instruction_limit;
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(HOOK_STEP),
            move |_lua, _debug| {
                let prev = counter_hook.fetch_add(HOOK_STEP as u64, Ordering::Relaxed);
                if prev + HOOK_STEP as u64 > max_instructions {
                    Err(mlua::Error::RuntimeError(
                        "extension execution aborted: instruction limit exceeded".to_string(),
                    ))
                } else {
                    Ok(VmState::Continue)
                }
            },
        );

        // Apply memory limit after setting up the host tables.
        lua.set_memory_limit(self.memory_limit)
            .map_err(|e| anyhow!("failed to set Lua memory limit for '{id}': {e}"))?;

        // Execute init script.
        instruction_count.store(0, Ordering::Relaxed);
        lua.load(&source)
            .set_name(format!("@{id}/{}", discovered.manifest.main))
            .exec()
            .map_err(|e| anyhow!("failed to execute extension '{id}': {e}"))?;

        self.loaded.insert(
            id,
            LoadedExtension {
                manifest: discovered.manifest,
                dir: discovered.dir,
                lua,
                callbacks,
                instruction_count,
            },
        );

        Ok(())
    }
}

fn check_perm(perms: &HashSet<Permission>, perm: Permission) -> mlua::Result<()> {
    if perms.contains(&perm) {
        Ok(())
    } else {
        Err(mlua::Error::RuntimeError(format!(
            "permission denied: {}",
            perm.as_str()
        )))
    }
}

fn build_safe_os(lua: &Lua) -> mlua::Result<Table> {
    let os = lua.create_table()?;
    let start = Instant::now();

    os.set(
        "time",
        lua.create_function(|_, ()| {
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            Ok(secs)
        })?,
    )?;

    os.set(
        "clock",
        lua.create_function(move |_, ()| Ok(start.elapsed().as_secs_f64()))?,
    )?;

    os.set(
        "date",
        lua.create_function(|lua, (fmt, ts): (Option<String>, Option<i64>)| {
            let secs = ts.unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0)
            });
            let (year, month, day, hour, min, sec, yday, wday) = unix_to_utc_parts(secs);
            let fmt = fmt.unwrap_or_else(|| "%Y-%m-%d %H:%M:%S".to_string());
            let clean = fmt.strip_prefix('!').unwrap_or(&fmt);
            if clean == "*t" {
                let tbl = lua.create_table()?;
                tbl.set("year", year)?;
                tbl.set("month", month)?;
                tbl.set("day", day)?;
                tbl.set("hour", hour)?;
                tbl.set("min", min)?;
                tbl.set("sec", sec)?;
                tbl.set("yday", yday)?;
                tbl.set("wday", wday)?;
                tbl.set("isdst", false)?;
                return Ok(Value::Table(tbl));
            }
            let rendered = clean
                .replace("%Y", &format!("{year:04}"))
                .replace("%m", &format!("{month:02}"))
                .replace("%d", &format!("{day:02}"))
                .replace("%H", &format!("{hour:02}"))
                .replace("%M", &format!("{min:02}"))
                .replace("%S", &format!("{sec:02}"))
                .replace("%F", &format!("{year:04}-{month:02}-{day:02}"))
                .replace("%T", &format!("{hour:02}:{min:02}:{sec:02}"))
                .replace(
                    "%c",
                    &format!("{year:04}-{month:02}-{day:02} {hour:02}:{min:02}:{sec:02}"),
                );
            Ok(Value::String(lua.create_string(&rendered)?))
        })?,
    )?;

    Ok(os)
}

/// Howard Hinnant's civil-from-days conversion (UTC) so `os.date` needs no external crate.
fn unix_to_utc_parts(secs: i64) -> (i64, u32, u32, u32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400) as u32;
    let hour = rem / 3_600;
    let min = (rem % 3_600) / 60;
    let sec = rem % 60;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    let wday = ((days + 4).rem_euclid(7) + 1) as u32; // 1 = Sunday .. 7 = Saturday
    let yday = (doy as u32) + 1;

    (year, m as u32, d as u32, hour, min, sec, yday, wday)
}

fn build_context_table(lua: &Lua, ctx: &EditorContext) -> mlua::Result<Table> {
    let tbl = lua.create_table()?;
    if let Some(root) = &ctx.workspace_root {
        tbl.set("workspace_root", root.to_string_lossy().as_ref())?;
    }
    if let Some(path) = &ctx.active_path {
        tbl.set("active_path", path.to_string_lossy().as_ref())?;
    }
    if let Some(text) = &ctx.active_text {
        tbl.set("active_text", text.as_str())?;
    }
    if let Some(sel) = &ctx.selection {
        tbl.set("selection", sel.as_str())?;
    }
    Ok(tbl)
}

fn parse_panel_value(default_title: &str, val: Value) -> anyhow::Result<PanelContent> {
    let Value::Table(tbl) = val else {
        bail!("panel render function must return a table");
    };

    let title: String = tbl
        .get::<Option<String>>("title")?
        .unwrap_or_else(|| default_title.to_string());

    let list_tbl: Table = if let Some(items) = tbl.get::<Option<Table>>("items")? {
        items
    } else if let Some(lines) = tbl.get::<Option<Table>>("lines")? {
        lines
    } else {
        tbl
    };

    let mut items = Vec::new();
    for entry in list_tbl.sequence_values::<Value>() {
        match entry? {
            Value::String(s) => {
                items.push(PanelItem {
                    label: s.to_str()?.to_string(),
                    detail: None,
                    command: None,
                });
            }
            Value::Table(item_tbl) => {
                let label: String = item_tbl.get("label")?;
                let detail: Option<String> = item_tbl.get("detail")?;
                let command: Option<String> = item_tbl.get("command")?;
                items.push(PanelItem {
                    label,
                    detail,
                    command,
                });
            }
            other => {
                bail!("unsupported panel item type: {}", other.type_name());
            }
        }
    }

    Ok(PanelContent { title, items })
}

fn resolve_confined_read_path(root: &Path, requested: &str) -> mlua::Result<PathBuf> {
    let canon_root = root
        .canonicalize()
        .map_err(|e| mlua::Error::RuntimeError(format!("invalid workspace root: {e}")))?;
    let candidate = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        canon_root.join(requested)
    };
    let canon_target = candidate
        .canonicalize()
        .map_err(|e| mlua::Error::RuntimeError(format!("cannot resolve path '{requested}': {e}")))?;
    if !canon_target.starts_with(&canon_root) {
        return Err(mlua::Error::RuntimeError(format!(
            "fs.read denied: path '{requested}' escapes workspace root"
        )));
    }
    Ok(canon_target)
}

fn resolve_confined_write_path(root: &Path, requested: &str) -> mlua::Result<PathBuf> {
    let canon_root = root
        .canonicalize()
        .map_err(|e| mlua::Error::RuntimeError(format!("invalid workspace root: {e}")))?;
    let req_path = Path::new(requested);
    if req_path
        .components()
        .any(|c| matches!(c, Component::ParentDir))
    {
        return Err(mlua::Error::RuntimeError(format!(
            "fs.write denied: '..' not allowed in path '{requested}'"
        )));
    }
    let candidate = if req_path.is_absolute() {
        req_path.to_path_buf()
    } else {
        canon_root.join(req_path)
    };

    // Canonicalize the nearest existing ancestor to prevent symlink escapes.
    let mut ancestor = candidate.as_path();
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let Some(name) = ancestor.file_name() else {
            return Err(mlua::Error::RuntimeError(format!(
                "fs.write denied: invalid path '{requested}'"
            )));
        };
        suffix.push(name.to_os_string());
        let Some(parent) = ancestor.parent() else {
            return Err(mlua::Error::RuntimeError(format!(
                "fs.write denied: invalid path '{requested}'"
            )));
        };
        ancestor = parent;
    }
    let mut resolved = ancestor
        .canonicalize()
        .map_err(|e| mlua::Error::RuntimeError(format!("cannot resolve path '{requested}': {e}")))?;
    for part in suffix.into_iter().rev() {
        resolved.push(part);
    }
    if !resolved.starts_with(&canon_root) {
        return Err(mlua::Error::RuntimeError(format!(
            "fs.write denied: path '{requested}' escapes workspace root"
        )));
    }
    Ok(resolved)
}

fn build_blur_api(
    lua: &Lua,
    ext_id: String,
    perms: Arc<HashSet<Permission>>,
    requests: Arc<Mutex<Vec<HostRequest>>>,
    context: Arc<Mutex<EditorContext>>,
    callbacks: Arc<Mutex<CallbackStore>>,
) -> mlua::Result<Table> {
    let blur = lua.create_table()?;

    // blur.register_command{ id, title, category?, run = fn }
    {
        let ext_id = ext_id.clone();
        let perms = Arc::clone(&perms);
        let requests = Arc::clone(&requests);
        let callbacks = Arc::clone(&callbacks);
        blur.set(
            "register_command",
            lua.create_function(move |lua, spec: Table| {
                check_perm(&perms, Permission::Commands)?;
                let id: String = spec.get("id")?;
                let title: String = spec.get("title")?;
                let category: String = spec
                    .get::<Option<String>>("category")?
                    .unwrap_or_else(|| "Extension".to_string());
                let run: Function = spec.get("run")?;
                let key = lua.create_registry_value(run)?;
                callbacks.lock().commands.insert(id.clone(), key);
                requests
                    .lock()
                    .push(HostRequest::RegisterCommand(CommandRegistration {
                        extension_id: ext_id.clone(),
                        id,
                        title,
                        category,
                    }));
                Ok(())
            })?,
        )?;
    }

    // blur.bind(keys, command_id)
    {
        let ext_id = ext_id.clone();
        let perms = Arc::clone(&perms);
        let requests = Arc::clone(&requests);
        blur.set(
            "bind",
            lua.create_function(move |_, (keys, command_id): (String, String)| {
                check_perm(&perms, Permission::Keymap)?;
                requests.lock().push(HostRequest::Bind {
                    extension_id: ext_id.clone(),
                    keys,
                    command_id,
                });
                Ok(())
            })?,
        )?;
    }

    // blur.notify(msg, level?)
    {
        let ext_id = ext_id.clone();
        let perms = Arc::clone(&perms);
        let requests = Arc::clone(&requests);
        blur.set(
            "notify",
            lua.create_function(move |_, (message, level): (String, Option<String>)| {
                check_perm(&perms, Permission::Ui)?;
                let level = match level.as_deref().map(str::to_ascii_lowercase).as_deref() {
                    Some("warn" | "warning") => NotifyLevel::Warning,
                    Some("error" | "err") => NotifyLevel::Error,
                    _ => NotifyLevel::Info,
                };
                requests.lock().push(HostRequest::Notify {
                    extension_id: ext_id.clone(),
                    message,
                    level,
                });
                Ok(())
            })?,
        )?;
    }

    // blur.log(msg)
    {
        let ext_id = ext_id.clone();
        let requests = Arc::clone(&requests);
        blur.set(
            "log",
            lua.create_function(move |_, message: String| {
                tracing::info!(extension = %ext_id, "{message}");
                requests.lock().push(HostRequest::Log {
                    extension_id: ext_id.clone(),
                    message,
                });
                Ok(())
            })?,
        )?;
    }

    // blur.register_panel{ id, title, render = fn }
    {
        let ext_id = ext_id.clone();
        let perms = Arc::clone(&perms);
        let requests = Arc::clone(&requests);
        let callbacks = Arc::clone(&callbacks);
        blur.set(
            "register_panel",
            lua.create_function(move |lua, spec: Table| {
                check_perm(&perms, Permission::Ui)?;
                let id: String = spec.get("id")?;
                let title: String = spec.get("title")?;
                let render: Function = spec.get("render")?;
                let key = lua.create_registry_value(render)?;
                callbacks
                    .lock()
                    .panels
                    .insert(id.clone(), (title.clone(), key));
                requests
                    .lock()
                    .push(HostRequest::RegisterPanel(PanelRegistration {
                        extension_id: ext_id.clone(),
                        id,
                        title,
                    }));
                Ok(())
            })?,
        )?;
    }

    // blur.register_theme{ name, base?, colors? }
    {
        let ext_id = ext_id.clone();
        let perms = Arc::clone(&perms);
        let requests = Arc::clone(&requests);
        blur.set(
            "register_theme",
            lua.create_function(move |_, spec: Table| {
                check_perm(&perms, Permission::Themes)?;
                let name: String = spec.get("name")?;
                let base: String = spec
                    .get::<Option<String>>("base")?
                    .unwrap_or_else(|| "dark".to_string());
                let mut colors = HashMap::new();
                if let Some(colors_tbl) = spec.get::<Option<Table>>("colors")? {
                    for pair in colors_tbl.pairs::<String, String>() {
                        let (k, v) = pair?;
                        colors.insert(k, v);
                    }
                }
                for pair in spec.pairs::<String, Value>() {
                    let (k, v) = pair?;
                    if k != "name" && k != "base" && k != "colors" {
                        if let Value::String(s) = v {
                            colors.insert(k, s.to_str()?.to_string());
                        }
                    }
                }
                requests
                    .lock()
                    .push(HostRequest::RegisterTheme(ThemeRegistration {
                        extension_id: ext_id.clone(),
                        name,
                        base,
                        colors,
                    }));
                Ok(())
            })?,
        )?;
    }

    // blur.on(event, fn)
    {
        let callbacks = Arc::clone(&callbacks);
        blur.set(
            "on",
            lua.create_function(move |lua, (event, func): (String, Function)| {
                let key = lua.create_registry_value(func)?;
                callbacks
                    .lock()
                    .events
                    .entry(event)
                    .or_default()
                    .push(key);
                Ok(())
            })?,
        )?;
    }

    // blur.workspace
    {
        let workspace = lua.create_table()?;
        let perms = Arc::clone(&perms);
        let context = Arc::clone(&context);
        workspace.set(
            "root",
            lua.create_function(move |_, ()| {
                check_perm(&perms, Permission::WorkspaceRead)?;
                Ok(context
                    .lock()
                    .workspace_root
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()))
            })?,
        )?;
        blur.set("workspace", workspace)?;
    }

    // blur.editor
    {
        let editor = lua.create_table()?;
        {
            let perms = Arc::clone(&perms);
            let context = Arc::clone(&context);
            editor.set(
                "active_path",
                lua.create_function(move |_, ()| {
                    check_perm(&perms, Permission::WorkspaceRead)?;
                    Ok(context
                        .lock()
                        .active_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned()))
                })?,
            )?;
        }
        {
            let perms = Arc::clone(&perms);
            let context = Arc::clone(&context);
            editor.set(
                "active_text",
                lua.create_function(move |_, ()| {
                    check_perm(&perms, Permission::WorkspaceRead)?;
                    Ok(context.lock().active_text.clone())
                })?,
            )?;
        }
        {
            let perms = Arc::clone(&perms);
            let context = Arc::clone(&context);
            editor.set(
                "selection",
                lua.create_function(move |_, ()| {
                    check_perm(&perms, Permission::WorkspaceRead)?;
                    Ok(context.lock().selection.clone())
                })?,
            )?;
        }
        {
            let ext_id = ext_id.clone();
            let perms = Arc::clone(&perms);
            let requests = Arc::clone(&requests);
            editor.set(
                "insert",
                lua.create_function(move |_, text: String| {
                    check_perm(&perms, Permission::Commands)?;
                    requests.lock().push(HostRequest::InsertText {
                        extension_id: ext_id.clone(),
                        text,
                    });
                    Ok(())
                })?,
            )?;
        }
        {
            let ext_id = ext_id.clone();
            let perms = Arc::clone(&perms);
            let requests = Arc::clone(&requests);
            editor.set(
                "open_file",
                lua.create_function(move |_, path: String| {
                    check_perm(&perms, Permission::Ui)?;
                    requests.lock().push(HostRequest::OpenFile {
                        extension_id: ext_id.clone(),
                        path: PathBuf::from(path),
                    });
                    Ok(())
                })?,
            )?;
        }
        blur.set("editor", editor)?;
    }

    // blur.fs
    {
        let fs_tbl = lua.create_table()?;
        {
            let perms = Arc::clone(&perms);
            let context = Arc::clone(&context);
            fs_tbl.set(
                "read",
                lua.create_function(move |_, path: String| {
                    check_perm(&perms, Permission::FsRead)?;
                    let root = context.lock().workspace_root.clone().ok_or_else(|| {
                        mlua::Error::RuntimeError(
                            "fs.read requires an open workspace_root".to_string(),
                        )
                    })?;
                    let target = resolve_confined_read_path(&root, &path)?;
                    std::fs::read_to_string(&target).map_err(|e| {
                        mlua::Error::RuntimeError(format!("fs.read failed for '{path}': {e}"))
                    })
                })?,
            )?;
        }
        {
            let perms = Arc::clone(&perms);
            let context = Arc::clone(&context);
            fs_tbl.set(
                "write",
                lua.create_function(move |_, (path, content): (String, String)| {
                    check_perm(&perms, Permission::FsWrite)?;
                    let root = context.lock().workspace_root.clone().ok_or_else(|| {
                        mlua::Error::RuntimeError(
                            "fs.write requires an open workspace_root".to_string(),
                        )
                    })?;
                    let target = resolve_confined_write_path(&root, &path)?;
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent).map_err(|e| {
                            mlua::Error::RuntimeError(format!(
                                "fs.write failed to create parent dirs: {e}"
                            ))
                        })?;
                    }
                    std::fs::write(&target, content).map_err(|e| {
                        mlua::Error::RuntimeError(format!("fs.write failed for '{path}': {e}"))
                    })
                })?,
            )?;
        }
        blur.set("fs", fs_tbl)?;
    }

    // blur.process
    {
        let proc_tbl = lua.create_table()?;
        let perms = Arc::clone(&perms);
        let context = Arc::clone(&context);
        proc_tbl.set(
            "run",
            lua.create_function(move |lua, (cmd, args): (String, Option<Vec<String>>)| {
                check_perm(&perms, Permission::Process)?;
                let mut command = Command::new(&cmd);
                if let Some(args) = args {
                    command.args(args);
                }
                if let Some(root) = context.lock().workspace_root.clone() {
                    command.current_dir(root);
                }
                let output = command.output().map_err(|e| {
                    mlua::Error::RuntimeError(format!("process.run '{cmd}' failed: {e}"))
                })?;
                let tbl = lua.create_table()?;
                tbl.set("status", output.status.code().unwrap_or(-1))?;
                tbl.set("stdout", String::from_utf8_lossy(&output.stdout).as_ref())?;
                tbl.set("stderr", String::from_utf8_lossy(&output.stderr).as_ref())?;
                Ok(tbl)
            })?,
        )?;
        blur.set("process", proc_tbl)?;
    }

    Ok(blur)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::AtomicUsize;

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn make_temp_dir(label: &str) -> PathBuf {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "blur-ext-test-{}-{}-{n}",
            std::process::id(),
            label
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_ext(root: &Path, id: &str, perms: &[&str], lua_src: &str) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        let perms_toml: Vec<String> = perms.iter().map(|p| format!("\"{p}\"")).collect();
        let manifest = format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"0.1.0\"\npermissions = [{}]\n",
            perms_toml.join(", ")
        );
        fs::write(dir.join("extension.toml"), manifest).unwrap();
        fs::write(dir.join("init.lua"), lua_src).unwrap();
    }

    #[test]
    fn sandbox_blocks_io_and_os_execute_but_allows_safe_os() {
        let root = make_temp_dir("sandbox");
        write_ext(
            &root,
            "sandbox-check",
            &["ui"],
            r#"
assert(io == nil, "io must be nil")
assert(package == nil, "package must be nil")
assert(debug == nil, "debug must be nil")
assert(os ~= nil, "safe os must exist")
assert(os.execute == nil, "os.execute must be nil")
assert(type(os.time()) == "number")
assert(type(os.clock()) == "number")
assert(type(os.date("%Y")) == "string")
blur.notify("sandbox ok", "info")
"#,
        );

        let mut granted = HashMap::new();
        granted.insert("sandbox-check".to_string(), vec![Permission::Ui]);
        let mut host = ExtensionHost::new(vec![root.clone()], granted);
        host.load("sandbox-check").unwrap();
        let reqs = host.drain_requests();
        assert!(reqs.iter().any(|r| matches!(
            r,
            HostRequest::Notify { message, level: NotifyLevel::Info, .. } if message == "sandbox ok"
        )));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn permission_denial_raises_lua_error() {
        let root = make_temp_dir("perm-deny");
        write_ext(
            &root,
            "greedy",
            &["fs.read"],
            r#"local _ = blur.fs.read("secret.txt")"#,
        );

        // Host does NOT grant FsRead
        let mut host = ExtensionHost::new(vec![root.clone()], HashMap::new());
        let err = host.load("greedy").unwrap_err().to_string();
        assert!(
            err.contains("permission denied: fs.read"),
            "unexpected error: {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn command_panel_and_event_handlers_work() {
        let root = make_temp_dir("lifecycle");
        write_ext(
            &root,
            "demo",
            &["commands", "keymap", "ui", "workspace.read"],
            r#"
local opened = {}

blur.register_command{
    id = "demo.greet",
    title = "Greet",
    category = "Demo",
    run = function(ctx)
        blur.editor.insert("Hello " .. (ctx.selection or "world"))
        blur.notify("greeted", "info")
    end
}
blur.bind("ctrl-alt-g", "demo.greet")

blur.register_panel{
    id = "demo.panel",
    title = "Demo Panel",
    render = function()
        return {
            items = {
                { label = "First", detail = "one", command = "demo.greet" },
                "Plain string item",
            }
        }
    end
}

blur.on("file_opened", function(payload)
    table.insert(opened, payload.path)
    blur.notify("opened:" .. payload.path, "info")
end)
"#,
        );

        let mut granted = HashMap::new();
        granted.insert(
            "demo".to_string(),
            vec![
                Permission::Commands,
                Permission::Keymap,
                Permission::Ui,
                Permission::WorkspaceRead,
            ],
        );
        let mut host = ExtensionHost::new(vec![root.clone()], granted);
        host.set_context(EditorContext {
            workspace_root: Some(root.clone()),
            active_path: Some(root.join("a.rs")),
            active_text: Some("fn main() {}".into()),
            selection: Some("Blur".into()),
        });

        host.load("demo").unwrap();
        let init_reqs = host.drain_requests();
        assert!(init_reqs.iter().any(|r| matches!(r, HostRequest::RegisterCommand(c) if c.id == "demo.greet")));
        assert!(init_reqs.iter().any(|r| matches!(r, HostRequest::Bind { keys, .. } if keys == "ctrl-alt-g")));
        assert!(init_reqs.iter().any(|r| matches!(r, HostRequest::RegisterPanel(p) if p.id == "demo.panel")));

        host.run_command("demo.greet").unwrap();
        let cmd_reqs = host.drain_requests();
        assert!(cmd_reqs.iter().any(|r| matches!(r, HostRequest::InsertText { text, .. } if text == "Hello Blur")));
        assert!(cmd_reqs.iter().any(|r| matches!(r, HostRequest::Notify { message, .. } if message == "greeted")));

        let panel = host.render_panel("demo.panel").unwrap();
        assert_eq!(panel.title, "Demo Panel");
        assert_eq!(panel.items.len(), 2);
        assert_eq!(panel.items[0].label, "First");
        assert_eq!(panel.items[1].label, "Plain string item");

        host.emit("file_opened", serde_json::json!({ "path": "src/main.rs" }))
            .unwrap();
        let ev_reqs = host.drain_requests();
        assert!(ev_reqs.iter().any(|r| matches!(r, HostRequest::Notify { message, .. } if message == "opened:src/main.rs")));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn runaway_loop_is_aborted_by_instruction_hook() {
        let root = make_temp_dir("runaway");
        write_ext(&root, "looper", &[], "while true do end");

        let mut host = ExtensionHost::new(vec![root.clone()], HashMap::new());
        host.set_instruction_limit(100_000);
        let err = host.load("looper").unwrap_err().to_string();
        assert!(
            err.contains("instruction limit exceeded"),
            "unexpected error: {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn memory_limit_aborts_unbounded_allocation() {
        let root = make_temp_dir("memlimit");
        write_ext(
            &root,
            "oom",
            &[],
            r#"
local t = {}
local chunk = string.rep("x", 65536)
for i = 1, 1000 do
    t[i] = chunk .. tostring(i)
end
"#,
        );

        let mut host = ExtensionHost::new(vec![root.clone()], HashMap::new());
        host.set_memory_limit(1024 * 1024); // 1 MiB
        let err = host.load("oom").unwrap_err();
        assert!(
            err.to_string().to_lowercase().contains("memory"),
            "expected memory error, got: {err}"
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn fs_read_and_write_are_confined_to_workspace_root() {
        let root = make_temp_dir("fs-confine");
        let ws = root.join("workspace");
        fs::create_dir_all(&ws).unwrap();
        let outside = root.join("outside.txt");
        fs::write(&outside, "secret").unwrap();

        write_ext(
            &root,
            "fs-ext",
            &["fs.read", "fs.write", "commands"],
            r#"
blur.register_command{
    id = "fs.ok",
    title = "Write and Read",
    run = function()
        blur.fs.write("notes.txt", "hello")
        assert(blur.fs.read("notes.txt") == "hello")
    end
}
blur.register_command{
    id = "fs.escape",
    title = "Escape",
    run = function()
        blur.fs.read("../outside.txt")
    end
}
"#,
        );

        let mut granted = HashMap::new();
        granted.insert(
            "fs-ext".to_string(),
            vec![Permission::FsRead, Permission::FsWrite, Permission::Commands],
        );
        let mut host = ExtensionHost::new(vec![root.clone()], granted);
        host.set_context(EditorContext {
            workspace_root: Some(ws.clone()),
            ..Default::default()
        });
        host.load("fs-ext").unwrap();
        host.run_command("fs.ok").unwrap();
        let err = host.run_command("fs.escape").unwrap_err().to_string();
        assert!(err.contains("escapes workspace root"), "unexpected: {err}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn loads_sample_hello_blur_extension() {
        let sample_extensions_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("extensions");
        let mut granted = HashMap::new();
        granted.insert(
            "hello-blur".to_string(),
            vec![
                Permission::Commands,
                Permission::Keymap,
                Permission::Ui,
                Permission::WorkspaceRead,
            ],
        );
        let mut host = ExtensionHost::new(vec![sample_extensions_dir], granted);
        let discovered = host.discover();
        assert!(
            discovered.iter().any(|d| d.manifest.id == "hello-blur"),
            "hello-blur sample extension should be discovered"
        );

        host.load("hello-blur").unwrap();
        let _ = host.drain_requests();

        host.run_command("hello-blur.say-hello").unwrap();
        let reqs = host.drain_requests();
        assert!(reqs.iter().any(|r| matches!(r, HostRequest::Notify { .. })));

        let panel = host.render_panel("hello-blur.panel").unwrap();
        assert!(!panel.items.is_empty());

        host.emit("file_opened", serde_json::json!({ "path": "README.md" }))
            .unwrap();
        let panel_after = host.render_panel("hello-blur.panel").unwrap();
        assert!(panel_after
            .items
            .iter()
            .any(|item| item.label.contains("README.md")));
    }
}
