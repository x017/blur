//! Merge conflict marker parser and resolver (supports 2-way and diff3 markers).

/// A parsed merge conflict region in a text file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRegion {
    /// 0-based line index of the `<<<<<<<` marker.
    pub start_line: usize,
    /// 0-based line index of the `>>>>>>>` marker (inclusive).
    pub end_line: usize,
    /// Content from the "ours" side.
    pub ours: String,
    /// Optional common ancestor content from diff3 (`|||||||`) markers.
    pub base: Option<String>,
    /// Content from the "theirs" side.
    pub theirs: String,
    /// Label following `<<<<<<<`.
    pub ours_label: String,
    /// Label following `>>>>>>>`.
    pub theirs_label: String,
}

/// Resolution choice for a [`ConflictRegion`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Ours,
    Theirs,
    Both,
    Base,
    Custom(String),
}

/// Parse all 2-way and diff3 conflict regions in `text`.
pub fn parse(text: &str) -> Vec<ConflictRegion> {
    let lines: Vec<&str> = text.lines().collect();
    let mut regions = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if let Some(rest) = lines[i].strip_prefix("<<<<<<<") {
            let start_line = i;
            let ours_label = rest.trim().to_string();
            i += 1;

            let mut ours_lines = Vec::new();
            let mut base_lines: Option<Vec<&str>> = None;
            let mut theirs_lines = Vec::new();

            #[derive(PartialEq)]
            enum Section {
                Ours,
                Base,
                Theirs,
            }
            let mut section = Section::Ours;
            let mut found_end = None;

            while i < lines.len() {
                let line = lines[i];
                if line.starts_with("|||||||") && section == Section::Ours {
                    section = Section::Base;
                    base_lines = Some(Vec::new());
                } else if line.starts_with("=======")
                    && (section == Section::Ours || section == Section::Base)
                {
                    section = Section::Theirs;
                } else if let Some(end_rest) = line.strip_prefix(">>>>>>>") {
                    if section == Section::Theirs {
                        found_end = Some((i, end_rest.trim().to_string()));
                        break;
                    }
                } else {
                    match section {
                        Section::Ours => ours_lines.push(line),
                        Section::Base => {
                            if let Some(ref mut b) = base_lines {
                                b.push(line);
                            }
                        }
                        Section::Theirs => theirs_lines.push(line),
                    }
                }
                i += 1;
            }

            if let Some((end_line, theirs_label)) = found_end {
                regions.push(ConflictRegion {
                    start_line,
                    end_line,
                    ours: ours_lines.join("\n"),
                    base: base_lines.map(|b| b.join("\n")),
                    theirs: theirs_lines.join("\n"),
                    ours_label,
                    theirs_label,
                });
                i = end_line + 1;
            }
        } else {
            i += 1;
        }
    }

    regions
}

/// Resolve the `region_index`-th conflict in `text` using `resolution`.
///
/// If `region_index` is out of bounds, returns `text` unchanged.
pub fn resolve(text: &str, region_index: usize, resolution: Resolution) -> String {
    let regions = parse(text);
    let Some(region) = regions.get(region_index) else {
        return text.to_string();
    };

    let trailing_newline = text.ends_with('\n');
    let lines: Vec<&str> = text.lines().collect();

    let replacement: String = match resolution {
        Resolution::Ours => region.ours.clone(),
        Resolution::Theirs => region.theirs.clone(),
        Resolution::Both => match (region.ours.is_empty(), region.theirs.is_empty()) {
            (true, true) => String::new(),
            (true, false) => region.theirs.clone(),
            (false, true) => region.ours.clone(),
            (false, false) => format!("{}\n{}", region.ours, region.theirs),
        },
        Resolution::Base => region.base.clone().unwrap_or_default(),
        Resolution::Custom(s) => s,
    };

    let mut out_lines: Vec<&str> = Vec::with_capacity(lines.len());
    out_lines.extend_from_slice(&lines[..region.start_line]);
    if !replacement.is_empty() {
        out_lines.push(&replacement);
    }
    if region.end_line + 1 < lines.len() {
        out_lines.extend_from_slice(&lines[region.end_line + 1..]);
    }

    let mut result = out_lines.join("\n");
    if trailing_newline && !result.is_empty() {
        result.push('\n');
    }
    result
}
