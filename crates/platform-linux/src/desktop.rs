//! Freedesktop desktop entries: parsing and the `Exec` command line.
//!
//! See the Desktop Entry Specification. Only the `[Desktop Entry]` group is
//! read; localized keys are ignored in favour of the unlocalized value, since
//! identity must not depend on the current locale.

use std::collections::HashMap;

/// The fields of a desktop entry that identity and launching need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopEntry {
    pub name: String,
    /// The command line with field codes removed; never run through a shell.
    pub exec: Vec<String>,
    pub try_exec: Option<String>,
    /// Not shown in menus: typically a helper, daemon or handler.
    pub no_display: bool,
    /// Treated as deleted by the spec.
    pub hidden: bool,
    /// The Flatpak application ID, from `X-Flatpak`.
    pub flatpak_id: Option<String>,
    /// The Snap instance name, from `X-SnapInstanceName`.
    pub snap_name: Option<String>,
}

/// Why an entry is not a launchable application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryError {
    MissingGroup,
    NotAnApplication,
    MissingName,
    MissingExec,
    InvalidExec,
}

/// Parses an application desktop entry.
///
/// # Errors
///
/// [`EntryError`] when the text is not a valid `Type=Application` entry.
pub fn parse_entry(text: &str) -> Result<DesktopEntry, EntryError> {
    let mut keys: HashMap<&str, &str> = HashMap::new();
    let mut in_main_group = false;
    let mut saw_group = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_main_group = line == "[Desktop Entry]";
            saw_group |= in_main_group;
            continue;
        }
        if !in_main_group {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            // Unlocalized keys only (`Name`, not `Name[de]`); the first value wins.
            if !key.contains('[') {
                keys.entry(key).or_insert_with(|| value.trim());
            }
        }
    }
    if !saw_group {
        return Err(EntryError::MissingGroup);
    }
    if keys.get("Type").copied() != Some("Application") {
        return Err(EntryError::NotAnApplication);
    }
    let name = keys
        .get("Name")
        .map(|name| unescape_string(name))
        .filter(|name| !name.is_empty());
    let name = name.ok_or(EntryError::MissingName)?;
    let exec = keys.get("Exec").ok_or(EntryError::MissingExec)?;
    let flag = |key: &str| keys.get(key).is_some_and(|value| *value == "true");
    Ok(DesktopEntry {
        name,
        exec: parse_exec(&unescape_string(exec)).ok_or(EntryError::InvalidExec)?,
        try_exec: keys.get("TryExec").map(|value| unescape_string(value)),
        no_display: flag("NoDisplay"),
        hidden: flag("Hidden"),
        flatpak_id: keys.get("X-Flatpak").map(|value| (*value).to_owned()),
        snap_name: keys
            .get("X-SnapInstanceName")
            .map(|value| (*value).to_owned()),
    })
}

/// Undoes the `string` value escapes (`\s`, `\n`, `\t`, `\r`, `\\`).
fn unescape_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('s') => out.push(' '),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Splits an `Exec` value into arguments per the spec's quoting rules and
/// removes field codes (`%f %F %u %U` and deprecated ones) so the result can
/// launch the app with no files. `%%` becomes `%`. Flatpak's `@@`/`@@u`
/// file-forwarding markers are removed too. Returns `None` for unbalanced
/// quotes, an empty command, or an unknown field code.
#[must_use]
pub fn parse_exec(exec: &str) -> Option<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_arg = false;
    let mut quoted = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                in_arg = true;
            }
            '\\' if quoted => match chars.next() {
                Some(escaped @ ('"' | '`' | '$' | '\\')) => current.push(escaped),
                _ => return None,
            },
            ' ' | '\t' if !quoted => {
                if in_arg {
                    args.push(std::mem::take(&mut current));
                    in_arg = false;
                }
            }
            '%' => {
                in_arg = true;
                match chars.next() {
                    Some('%') => current.push('%'),
                    // File, URL, icon, name and location codes expand to nothing
                    // when launching without files; deprecated ones are removed.
                    Some(
                        'f' | 'F' | 'u' | 'U' | 'i' | 'c' | 'k' | 'd' | 'D' | 'n' | 'N' | 'v' | 'm',
                    ) => {}
                    _ => return None,
                }
            }
            other => {
                current.push(other);
                in_arg = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if in_arg {
        args.push(current);
    }
    args.retain(|arg| !arg.is_empty() && arg != "@@" && arg != "@@u");
    if args.is_empty() { None } else { Some(args) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_application_entry() {
        let entry = parse_entry(
            "# comment\n[Desktop Entry]\nType=Application\nName=Files\nName[de]=Dateien\n\
             Exec=nautilus --new-window %U\nNoDisplay=false\n\n[Desktop Action new]\nExec=other\n",
        )
        .unwrap();
        assert_eq!(entry.name, "Files");
        assert_eq!(entry.exec, ["nautilus", "--new-window"]);
        assert!(!entry.no_display && !entry.hidden);
    }

    #[test]
    fn reads_flatpak_and_snap_markers() {
        let flatpak = parse_entry(
            "[Desktop Entry]\nType=Application\nName=Edge\nX-Flatpak=com.microsoft.Edge\n\
             Exec=/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=/app/bin/edge \
             --file-forwarding com.microsoft.Edge @@u %U @@\n",
        )
        .unwrap();
        assert_eq!(flatpak.flatpak_id.as_deref(), Some("com.microsoft.Edge"));
        assert_eq!(
            flatpak.exec.last().map(String::as_str),
            Some("com.microsoft.Edge")
        );
        let snap = parse_entry(
            "[Desktop Entry]\nType=Application\nName=S\nExec=/snap/bin/s\nX-SnapInstanceName=s\n",
        )
        .unwrap();
        assert_eq!(snap.snap_name.as_deref(), Some("s"));
    }

    #[test]
    fn rejects_non_applications_and_missing_fields() {
        assert_eq!(
            parse_entry("Type=Application\n"),
            Err(EntryError::MissingGroup)
        );
        assert_eq!(
            parse_entry("[Desktop Entry]\nType=Link\nName=x\n"),
            Err(EntryError::NotAnApplication)
        );
        assert_eq!(
            parse_entry("[Desktop Entry]\nType=Application\nExec=x\n"),
            Err(EntryError::MissingName)
        );
        assert_eq!(
            parse_entry("[Desktop Entry]\nType=Application\nName=x\n"),
            Err(EntryError::MissingExec)
        );
    }

    #[test]
    fn exec_quoting_and_field_codes() {
        assert_eq!(
            parse_exec(r#""/opt/My App/bin/app" --flag "a \"q\" b" %f"#).unwrap(),
            ["/opt/My App/bin/app", "--flag", r#"a "q" b"#]
        );
        assert_eq!(
            parse_exec("app --percent=100%%").unwrap(),
            ["app", "--percent=100%"]
        );
        assert_eq!(parse_exec("app %U").unwrap(), ["app"]);
        assert_eq!(parse_exec("app --file=%f").unwrap(), ["app", "--file="]);
    }

    #[test]
    fn invalid_exec_is_rejected() {
        assert_eq!(parse_exec(r#"app "unclosed"#), None);
        assert_eq!(parse_exec("app %z"), None);
        assert_eq!(parse_exec("%U"), None);
        assert_eq!(parse_exec(r#""bad \q escape""#), None);
    }

    #[test]
    fn string_escapes_apply_before_exec_quoting() {
        let entry = parse_entry(
            "[Desktop Entry]\nType=Application\nName=x\nExec=\"/opt/a\\\\\\\\ b/app\"\n",
        )
        .unwrap();
        assert_eq!(entry.exec, ["/opt/a\\ b/app"]);
    }
}
