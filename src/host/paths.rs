//! Web1 filesystem path rules (spec §13.2).

/// Collapse repeated separators and `.` components of an absolute path.
pub fn normalize_components(abs: &str) -> String {
    let comps: Vec<&str> = abs.split('/').filter(|c| !c.is_empty() && *c != ".").collect();
    format!("/{}", comps.join("/"))
}

#[derive(Debug, PartialEq, Eq)]
pub enum PathError {
    Invalid(&'static str),
}

/// Resolve a Kiri path against the filesystem root. Returns the normalized
/// absolute path of a regular-file target.
pub fn resolve(root: &str, path: &str) -> Result<String, PathError> {
    if path.is_empty() {
        return Err(PathError::Invalid("empty_path"));
    }
    if path.contains('\0') {
        return Err(PathError::Invalid("nul"));
    }
    if path.split('/').any(|c| c == "..") {
        return Err(PathError::Invalid("parent_component"));
    }
    if path.ends_with('/') {
        return Err(PathError::Invalid("directory"));
    }
    let abs = if path.starts_with('/') { path.to_string() } else { format!("{root}/{path}") };
    let n = normalize_components(&abs);
    if n == "/" {
        return Err(PathError::Invalid("directory"));
    }
    Ok(n)
}

/// Component-based containment check (no string-prefix authorization).
pub fn within(path: &str, root: &str) -> bool {
    let p: Vec<&str> = path.split('/').filter(|c| !c.is_empty()).collect();
    let r: Vec<&str> = root.split('/').filter(|c| !c.is_empty()).collect();
    p.len() > r.len() && p[..r.len()] == r[..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules() {
        assert_eq!(resolve("/r", "/fixtures/./out").unwrap(), "/fixtures/out");
        assert_eq!(resolve("/r", "/fixtures//out").unwrap(), "/fixtures/out");
        assert_eq!(resolve("/r", "a/b").unwrap(), "/r/a/b");
        assert!(resolve("/r", "a/../b").is_err());
        assert!(resolve("/r", "a/").is_err());
        assert!(within("/fixtures/out", "/fixtures"));
        assert!(!within("/fixturesx/out", "/fixtures"));
        assert!(!within("/fixtures", "/fixtures"));
    }
}
