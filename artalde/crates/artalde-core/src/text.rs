//! Small string helpers.

/// The first line of `s`, trimmed and cut to `max` characters with `…`.
pub fn clip(s: &str, max: usize) -> String {
    let line = s.trim().lines().next().unwrap_or("").trim();
    if line.chars().count() <= max {
        return line.to_owned();
    }
    let mut out: String = line.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// `s` cut to `max` characters with `…`, keeping newlines.
pub fn clip_block(s: &str, max: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Compact age: `45s`, `12m`, `3h`, `2d`.
pub fn ago(secs: u64) -> String {
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
}

/// `s` quoted for a POSIX shell, so it is always one word.
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./@:=+,%".contains(c))
    {
        return s.to_owned();
    }
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clip_is_char_safe() {
        assert_eq!(clip("añoñoño", 4), "año…");
        assert_eq!(clip("  short\nsecond", 10), "short");
    }

    #[test]
    fn ago_buckets() {
        assert_eq!(ago(5), "5s");
        assert_eq!(ago(125), "2m");
        assert_eq!(ago(7300), "2h");
        assert_eq!(ago(200_000), "2d");
    }

    #[test]
    fn shell_quote_makes_one_word() {
        assert_eq!(shell_quote("data@fix-x"), "data@fix-x");
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's; rm -rf ~"), r"'it'\''s; rm -rf ~'");
        assert_eq!(shell_quote(""), "''");
    }
}
