//! Uniform tree representation for JSON, YAML, and TOML documents.

/// Value kind of a [`DataNode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

/// A node in a parsed JSON/YAML/TOML document tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataNode {
    /// Object key or `None` for the root / array elements.
    pub key: Option<String>,
    pub kind: DataKind,
    /// Leaf value rendered as a string (for primitives); `None` for containers.
    pub value: Option<String>,
    /// Child nodes (for `Object` and `Array`).
    pub children: Vec<DataNode>,
}

/// Parse a JSON document into a [`DataNode`] tree.
pub fn parse_json(src: &str) -> anyhow::Result<DataNode> {
    let val: serde_json::Value = serde_json::from_str(src)?;
    Ok(from_json(None, &val))
}

fn from_json(key: Option<String>, val: &serde_json::Value) -> DataNode {
    match val {
        serde_json::Value::Null => DataNode {
            key,
            kind: DataKind::Null,
            value: Some("null".to_string()),
            children: Vec::new(),
        },
        serde_json::Value::Bool(b) => DataNode {
            key,
            kind: DataKind::Bool,
            value: Some(b.to_string()),
            children: Vec::new(),
        },
        serde_json::Value::Number(n) => DataNode {
            key,
            kind: DataKind::Number,
            value: Some(n.to_string()),
            children: Vec::new(),
        },
        serde_json::Value::String(s) => DataNode {
            key,
            kind: DataKind::String,
            value: Some(s.clone()),
            children: Vec::new(),
        },
        serde_json::Value::Array(arr) => DataNode {
            key,
            kind: DataKind::Array,
            value: None,
            children: arr.iter().map(|v| from_json(None, v)).collect(),
        },
        serde_json::Value::Object(map) => DataNode {
            key,
            kind: DataKind::Object,
            value: None,
            children: map
                .iter()
                .map(|(k, v)| from_json(Some(k.clone()), v))
                .collect(),
        },
    }
}

/// Parse a YAML document into a [`DataNode`] tree.
pub fn parse_yaml(src: &str) -> anyhow::Result<DataNode> {
    let val: serde_yaml::Value = serde_yaml::from_str(src)?;
    Ok(from_yaml(None, &val))
}

fn from_yaml(key: Option<String>, val: &serde_yaml::Value) -> DataNode {
    match val {
        serde_yaml::Value::Null => DataNode {
            key,
            kind: DataKind::Null,
            value: Some("null".to_string()),
            children: Vec::new(),
        },
        serde_yaml::Value::Bool(b) => DataNode {
            key,
            kind: DataKind::Bool,
            value: Some(b.to_string()),
            children: Vec::new(),
        },
        serde_yaml::Value::Number(n) => DataNode {
            key,
            kind: DataKind::Number,
            value: Some(n.to_string()),
            children: Vec::new(),
        },
        serde_yaml::Value::String(s) => DataNode {
            key,
            kind: DataKind::String,
            value: Some(s.clone()),
            children: Vec::new(),
        },
        serde_yaml::Value::Sequence(seq) => DataNode {
            key,
            kind: DataKind::Array,
            value: None,
            children: seq.iter().map(|v| from_yaml(None, v)).collect(),
        },
        serde_yaml::Value::Mapping(map) => DataNode {
            key,
            kind: DataKind::Object,
            value: None,
            children: map
                .iter()
                .map(|(k, v)| {
                    let key_str = match k {
                        serde_yaml::Value::String(s) => s.clone(),
                        other => serde_yaml::to_string(other)
                            .unwrap_or_default()
                            .trim()
                            .to_string(),
                    };
                    from_yaml(Some(key_str), v)
                })
                .collect(),
        },
        serde_yaml::Value::Tagged(tagged) => from_yaml(key, &tagged.value),
    }
}

/// Parse a TOML document into a [`DataNode`] tree.
pub fn parse_toml(src: &str) -> anyhow::Result<DataNode> {
    let val: toml::Value = toml::from_str(src)?;
    Ok(from_toml(None, &val))
}

fn from_toml(key: Option<String>, val: &toml::Value) -> DataNode {
    match val {
        toml::Value::String(s) => DataNode {
            key,
            kind: DataKind::String,
            value: Some(s.clone()),
            children: Vec::new(),
        },
        toml::Value::Integer(i) => DataNode {
            key,
            kind: DataKind::Number,
            value: Some(i.to_string()),
            children: Vec::new(),
        },
        toml::Value::Float(f) => DataNode {
            key,
            kind: DataKind::Number,
            value: Some(f.to_string()),
            children: Vec::new(),
        },
        toml::Value::Boolean(b) => DataNode {
            key,
            kind: DataKind::Bool,
            value: Some(b.to_string()),
            children: Vec::new(),
        },
        toml::Value::Datetime(dt) => DataNode {
            key,
            kind: DataKind::String,
            value: Some(dt.to_string()),
            children: Vec::new(),
        },
        toml::Value::Array(arr) => DataNode {
            key,
            kind: DataKind::Array,
            value: None,
            children: arr.iter().map(|v| from_toml(None, v)).collect(),
        },
        toml::Value::Table(tbl) => DataNode {
            key,
            kind: DataKind::Object,
            value: None,
            children: tbl
                .iter()
                .map(|(k, v)| from_toml(Some(k.clone()), v))
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_yaml_and_toml_trees() {
        let j = parse_json(r#"{"name":"blur","nums":[1,2],"ok":true,"nil":null}"#).unwrap();
        assert_eq!(j.kind, DataKind::Object);
        assert_eq!(j.children.len(), 4);

        let y = parse_yaml("name: blur\nitems:\n  - a\n  - b\n").unwrap();
        assert_eq!(y.kind, DataKind::Object);
        assert_eq!(y.children[1].kind, DataKind::Array);
        assert_eq!(y.children[1].children.len(), 2);

        let t = parse_toml("[pkg]\nname = \"blur\"\nversion = 1\n").unwrap();
        assert_eq!(t.kind, DataKind::Object);
        assert_eq!(t.children[0].key.as_deref(), Some("pkg"));
        assert_eq!(t.children[0].children.len(), 2);
    }
}
