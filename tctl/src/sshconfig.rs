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

    /// A complete new block, preceded by an empty line.
    pub fn render_new(&self) -> String {
        let identity = self.identity_file.display().to_string();
        let identity = if identity
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\\'))
        {
            format!(
                "\"{}\"",
                identity.replace('\\', "\\\\").replace('"', "\\\"")
            )
        } else {
            identity
        };
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

fn is_host_line(line: &str) -> Option<&str> {
    line.strip_prefix("Host ")
        .map(|rest| rest.split_whitespace().next().unwrap_or(""))
}

/// Does `text` contain an exact `Host <name>` line?
pub fn has_block(text: &str, host: &str) -> bool {
    text.lines().any(|l| l.strip_prefix("Host ") == Some(host))
}

/// Current `HostName` and `User` of the block, if present.
pub fn current_values(text: &str, host: &str) -> (Option<String>, Option<String>) {
    let mut in_block = false;
    let mut hostname = None;
    let mut user = None;
    for line in text.lines() {
        if let Some(h) = is_host_line(line) {
            in_block = h == host;
            continue;
        }
        if !in_block {
            continue;
        }
        let t = line.trim_start();
        if line.starts_with(char::is_whitespace) {
            if hostname.is_none() {
                if let Some(v) = t.strip_prefix("HostName ") {
                    hostname = Some(v.split_whitespace().next().unwrap_or("").to_string());
                }
            }
            if user.is_none() {
                if let Some(v) = t.strip_prefix("User ") {
                    user = Some(v.split_whitespace().next().unwrap_or("").to_string());
                }
            }
        }
    }
    (hostname, user)
}

/// Returns the updated config text: the block for `block.host` gets the new
/// HostName/User and its forward lines replaced by the configured set (emitted
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
        if let Some(h) = is_host_line(line) {
            if in_block && !forward_done {
                emit(&mut out, &mut forward_done);
            }
            in_block = h == block.host;
            forward_done = false;
            out.push(line.to_string());
            continue;
        }
        if in_block && line.starts_with(char::is_whitespace) {
            let t = line.trim_start();
            if t.starts_with("RemoteForward ") || t.starts_with("LocalForward ") {
                if !forward_done {
                    emit(&mut out, &mut forward_done);
                }
                continue;
            }
            if t.starts_with("HostName ") {
                out.push(replace_after(line, "HostName ", block.ip));
                continue;
            }
            if t.starts_with("User ") {
                out.push(replace_after(line, "User ", block.user));
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

/// Keeps the indentation of `line`, replaces `<keyword>…` by `<keyword><value>`.
fn replace_after(line: &str, keyword: &str, value: &str) -> String {
    let indent_len = line.len() - line.trim_start().len();
    format!("{}{}{}", &line[..indent_len], keyword, value)
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
        assert_eq!(
            current_values(&s3, "demo"),
            (Some("1.2.3.5".into()), Some("devbox".into()))
        );
        assert_eq!(
            current_values(&s3, "other"),
            (Some("10.0.0.1".into()), Some("someone".into()))
        );
    }

    #[test]
    fn forwards_without_existing_go_before_next_host() {
        let id = PathBuf::from("/k");
        let text = "Host demo\n    HostName 1.1.1.1\n    User devbox\n\nHost trailing\n    HostName 2.2.2.2\n";
        let lf = vec!["80 127.0.0.1:80".to_string()];
        let out = upsert_block(text, &block("1.1.1.1", &[], &lf, &id));
        assert_eq!(out, "Host demo\n    HostName 1.1.1.1\n    User devbox\n\n    LocalForward 80 127.0.0.1:80\nHost trailing\n    HostName 2.2.2.2\n");
    }
}
