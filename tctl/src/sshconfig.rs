//! Management of the per-VM `Host` block in the user's ssh client config.

use std::path::Path;

use crate::Result;

/// Parameters of the managed block.
#[derive(Debug, Clone)]
pub struct HostBlock<'a> {
    pub host: &'a str,
    pub ip: &'a str,
    pub user: &'a str,
    pub identity_file: &'a Path,
    pub remote_forward: &'a [String],
    pub local_forward: &'a [String],
}

impl HostBlock<'_> {
    /// `    RemoteForward …` / `    LocalForward …` lines, in that order.
    pub fn forward_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (kw, list) in [
            ("RemoteForward", self.remote_forward),
            ("LocalForward", self.local_forward),
        ] {
            for entry in list {
                let e = entry.trim();
                if !e.is_empty() {
                    lines.push(format!("    {kw} {e}"));
                }
            }
        }
        lines
    }

    /// The `IdentityFile` argument, quoted when the path needs it.
    fn identity_arg(&self) -> String {
        quote_arg(&self.identity_file.display().to_string())
    }

    /// A complete new block, preceded by an empty line.
    pub fn render_new(&self) -> String {
        let identity = self.identity_arg();
        let mut s = format!(
            "\nHost {}\n    HostName {}\n    User {}\n    IdentityFile {}\n    IdentitiesOnly yes\n    StrictHostKeyChecking no\n    UserKnownHostsFile /dev/null\n",
            self.host,
            self.ip,
            self.user,
            identity
        );
        for l in self.forward_lines() {
            s.push_str(&l);
            s.push('\n');
        }
        s
    }
}

/// Quotes an argument for ssh_config when it contains whitespace, quotes or
/// backslashes; otherwise returns it unchanged. Inverse of [`first_arg`].
fn quote_arg(value: &str) -> String {
    if value
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '"' | '\\'))
    {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

/// The first argument of a directive, with OpenSSH double-quoting and
/// backslash escapes removed.
fn first_arg(rest: &str) -> String {
    let rest = rest.trim_start();
    let mut chars = rest.chars();
    if rest.starts_with('"') {
        chars.next();
        let mut out = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                '"' => break,
                c => out.push(c),
            }
        }
        out
    } else {
        rest.split_whitespace().next().unwrap_or("").to_string()
    }
}

/// Splits a config line into its keyword and argument text the way OpenSSH
/// does: optional leading whitespace, case-insensitive keyword, then
/// whitespace and/or a single `=` before the arguments. Comments and blank
/// lines yield `None`.
fn directive(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let end = t
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(t.len());
    let (keyword, rest) = t.split_at(end);
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    Some((keyword, rest))
}

/// Argument text of `line` if its keyword is `keyword` (case-insensitive).
fn argument<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    directive(line).and_then(|(kw, rest)| kw.eq_ignore_ascii_case(keyword).then_some(rest))
}

enum Section<'a> {
    Host(Vec<&'a str>),
    Match,
}

/// `Host …` and `Match …` lines both end the previous section.
fn section_start(line: &str) -> Option<Section<'_>> {
    let (keyword, rest) = directive(line)?;
    if keyword.eq_ignore_ascii_case("host") {
        Some(Section::Host(rest.split_whitespace().collect()))
    } else if keyword.eq_ignore_ascii_case("match") {
        Some(Section::Match)
    } else {
        None
    }
}

/// The managed block is a `Host` line whose only pattern is `host`.
fn starts_block(line: &str, host: &str) -> bool {
    matches!(section_start(line), Some(Section::Host(patterns)) if patterns == [host])
}

/// Does `text` contain the managed `Host <name>` block?
pub fn has_block(text: &str, host: &str) -> bool {
    text.lines().any(|l| starts_block(l, host))
}

/// The values `tctl` maintains in the managed block, as currently on disk.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BlockValues {
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub identity_file: Option<String>,
}

impl BlockValues {
    /// True if the block already carries exactly these values.
    pub fn matches(&self, block: &HostBlock<'_>) -> bool {
        let identity = block.identity_file.display().to_string();
        self.hostname.as_deref() == Some(block.ip)
            && self.user.as_deref() == Some(block.user)
            && self.identity_file.as_deref() == Some(identity.as_str())
    }
}

/// Current `HostName`, `User` and `IdentityFile` of the block (first
/// occurrence each, like ssh itself), if present.
pub fn current_values(text: &str, host: &str) -> BlockValues {
    let mut in_block = false;
    let mut values = BlockValues::default();
    for line in text.lines() {
        if section_start(line).is_some() {
            in_block = starts_block(line, host);
            continue;
        }
        if !in_block {
            continue;
        }
        for (keyword, slot) in [
            ("HostName", &mut values.hostname),
            ("User", &mut values.user),
            ("IdentityFile", &mut values.identity_file),
        ] {
            if slot.is_none() {
                if let Some(rest) = argument(line, keyword) {
                    *slot = Some(first_arg(rest));
                }
            }
        }
    }
    values
}

/// Returns the updated config text: the block for `block.host` gets the new
/// HostName/User/IdentityFile and its forward lines replaced by the configured set (emitted
/// where the first old forward was, or at the end of the block). If the block
/// does not exist it is appended.
pub fn upsert_block(text: &str, block: &HostBlock) -> String {
    if !has_block(text, block.host) {
        let mut out = text.to_string();
        out.push_str(&block.render_new());
        return out;
    }

    let forwards = block.forward_lines();
    let mut out: Vec<String> = Vec::new();
    let mut in_block = false;
    let mut forward_done = false;

    let emit = |out: &mut Vec<String>, done: &mut bool| {
        out.extend(forwards.iter().cloned());
        *done = true;
    };

    for line in text.lines() {
        if section_start(line).is_some() {
            if in_block && !forward_done {
                emit(&mut out, &mut forward_done);
            }
            in_block = starts_block(line, block.host);
            forward_done = false;
            out.push(line.to_string());
            continue;
        }
        if in_block {
            if argument(line, "RemoteForward").is_some() || argument(line, "LocalForward").is_some()
            {
                if !forward_done {
                    emit(&mut out, &mut forward_done);
                }
                continue;
            }
            if argument(line, "HostName").is_some() {
                out.push(replace_after(line, "HostName", block.ip));
                continue;
            }
            if argument(line, "User").is_some() {
                out.push(replace_after(line, "User", block.user));
                continue;
            }
            if argument(line, "IdentityFile").is_some() {
                out.push(replace_after(line, "IdentityFile", &block.identity_arg()));
                continue;
            }
        }
        out.push(line.to_string());
    }
    if in_block && !forward_done {
        emit(&mut out, &mut forward_done);
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Keeps the indentation of `line`, replaces `<keyword>…` by `<keyword> <value>`.
fn replace_after(line: &str, keyword: &str, value: &str) -> String {
    let indent_len = line.len() - line.trim_start().len();
    format!("{}{} {}", &line[..indent_len], keyword, value)
}

/// Reads, updates and writes the ssh config (mode 0600). Returns `true` if the block already existed.
pub fn upsert_file(path: &Path, block: &HostBlock) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let identity_file = block.identity_file.to_string_lossy();
    if [block.host, block.ip, block.user, identity_file.as_ref()]
        .into_iter()
        .chain(block.remote_forward.iter().map(String::as_str))
        .chain(block.local_forward.iter().map(String::as_str))
        .any(|value| value.chars().any(|c| matches!(c, '\r' | '\n' | '\0')))
    {
        return Err(crate::Error::Config(
            "SSH directive values must not contain CR, LF or NUL characters.".into(),
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e.into()),
    };
    let existed = has_block(&text, block.host);
    std::fs::write(path, upsert_block(&text, block))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(existed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn block<'a>(ip: &'a str, rf: &'a [String], lf: &'a [String], id: &'a Path) -> HostBlock<'a> {
        HostBlock {
            host: "demo",
            ip,
            user: "devbox",
            identity_file: id,
            remote_forward: rf,
            local_forward: lf,
        }
    }

    #[test]
    fn append_then_update() {
        let id = PathBuf::from("/h/.ssh/key");
        let rf = vec!["1080 127.0.0.1:1080".to_string()];
        let base = "Host other\n    HostName 10.0.0.1\n    User someone\n";
        let s1 = upsert_block(base, &block("1.2.3.4", &rf, &[], &id));
        assert_eq!(
            s1,
            "Host other\n    HostName 10.0.0.1\n    User someone\n\nHost demo\n    HostName 1.2.3.4\n    User devbox\n    IdentityFile /h/.ssh/key\n    IdentitiesOnly yes\n    StrictHostKeyChecking no\n    UserKnownHostsFile /dev/null\n    RemoteForward 1080 127.0.0.1:1080\n"
        );
        let lf = vec!["5432 127.0.0.1:5432".to_string()];
        let s2 = upsert_block(&s1, &block("1.2.3.5", &rf, &lf, &id));
        assert!(s2.contains("    HostName 1.2.3.5\n"));
        assert!(s2.ends_with(
            "    RemoteForward 1080 127.0.0.1:1080\n    LocalForward 5432 127.0.0.1:5432\n"
        ));
        let s3 = upsert_block(&s2, &block("1.2.3.5", &[], &[], &id));
        assert!(!s3.contains("Forward"));
        let demo = current_values(&s3, "demo");
        assert_eq!(demo.hostname.as_deref(), Some("1.2.3.5"));
        assert_eq!(demo.user.as_deref(), Some("devbox"));
        assert_eq!(demo.identity_file.as_deref(), Some("/h/.ssh/key"));
        assert!(demo.matches(&block("1.2.3.5", &[], &[], &id)));
        let other = current_values(&s3, "other");
        assert_eq!(other.hostname.as_deref(), Some("10.0.0.1"));
        assert_eq!(other.user.as_deref(), Some("someone"));
        assert_eq!(other.identity_file, None);
    }

    #[test]
    fn identity_file_is_updated_in_place_and_quoted_when_needed() {
        let plain = PathBuf::from("/h/.ssh/key");
        let spaced = PathBuf::from("/h/my keys/id \"quoted\"");
        let s1 = upsert_block("", &block("1.1.1.1", &[], &[], &plain));
        assert!(s1.contains("    IdentityFile /h/.ssh/key\n"));

        let s2 = upsert_block(&s1, &block("1.1.1.1", &[], &[], &spaced));
        assert!(
            s2.contains("    IdentityFile \"/h/my keys/id \\\"quoted\\\"\"\n"),
            "{s2}"
        );
        assert_eq!(s2.matches("IdentityFile").count(), 1, "{s2}");
        let values = current_values(&s2, "demo");
        assert_eq!(
            values.identity_file.as_deref(),
            Some("/h/my keys/id \"quoted\"")
        );
        assert!(values.matches(&block("1.1.1.1", &[], &[], &spaced)));
        assert!(!values.matches(&block("1.1.1.1", &[], &[], &plain)));

        let s3 = upsert_block(&s2, &block("1.1.1.1", &[], &[], &plain));
        assert!(s3.contains("    IdentityFile /h/.ssh/key\n"), "{s3}");
    }

    #[test]
    fn forwards_without_existing_go_before_next_host() {
        let id = PathBuf::from("/k");
        let text = "Host demo\n    HostName 1.1.1.1\n    User devbox\n\nHost trailing\n    HostName 2.2.2.2\n";
        let lf = vec!["80 127.0.0.1:80".to_string()];
        let out = upsert_block(text, &block("1.1.1.1", &[], &lf, &id));
        assert_eq!(out, "Host demo\n    HostName 1.1.1.1\n    User devbox\n\n    LocalForward 80 127.0.0.1:80\nHost trailing\n    HostName 2.2.2.2\n");
    }

    #[test]
    fn match_and_unusual_host_lines_end_the_managed_block() {
        let id = PathBuf::from("/k");
        let rf = vec!["1080 127.0.0.1:1080".to_string()];
        let text = concat!(
            "Host demo\n    HostName 1.1.1.1\n    User devbox\n    LocalForward 1 127.0.0.1:1\n",
            "Match host *.corp\n    User corp\n    LocalForward 2 127.0.0.1:2\n",
            "\thost other\n    HostName 9.9.9.9\n    RemoteForward 3 127.0.0.1:3\n",
            "Host=third\n    User third\n",
        );
        let out = upsert_block(text, &block("5.5.5.5", &rf, &[], &id));
        assert_eq!(
            out,
            concat!(
                "Host demo\n    HostName 5.5.5.5\n    User devbox\n    RemoteForward 1080 127.0.0.1:1080\n",
                "Match host *.corp\n    User corp\n    LocalForward 2 127.0.0.1:2\n",
                "\thost other\n    HostName 9.9.9.9\n    RemoteForward 3 127.0.0.1:3\n",
                "Host=third\n    User third\n",
            )
        );
        let demo = current_values(&out, "demo");
        assert_eq!(demo.hostname.as_deref(), Some("5.5.5.5"));
        assert_eq!(demo.user.as_deref(), Some("devbox"));
        let other = current_values(&out, "other");
        assert_eq!(other.hostname.as_deref(), Some("9.9.9.9"));
        assert_eq!(other.user, None);
        assert!(has_block(&out, "third"));
    }

    #[test]
    fn multi_pattern_host_line_is_not_the_managed_block() {
        let id = PathBuf::from("/k");
        let text = "Host demo demo.local\n    HostName 1.1.1.1\n";
        assert!(!has_block(text, "demo"));
        let out = upsert_block(text, &block("2.2.2.2", &[], &[], &id));
        assert!(out.starts_with("Host demo demo.local\n    HostName 1.1.1.1\n\nHost demo\n"));
        assert_eq!(
            current_values(&out, "demo").hostname.as_deref(),
            Some("2.2.2.2")
        );
    }

    #[test]
    fn directives_are_case_insensitive_with_flexible_separators() {
        assert_eq!(
            argument("    hostname=1.2.3.4", "HostName"),
            Some("1.2.3.4")
        );
        assert_eq!(argument("\tUSER   bob", "User"), Some("bob"));
        assert_eq!(argument("    HostName 1.2.3.4", "Host"), None);
        assert_eq!(argument("# Host demo", "Host"), None);
        assert!(starts_block("HOST demo", "demo"));
        assert!(!starts_block("Host demo other", "demo"));
        assert!(matches!(section_start("Match all"), Some(Section::Match)));
    }
}
