//! 语义化版本解析与比较（semver 2.0.0 优先级规则的核心子集）。

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticVersion {
    major: u64,
    minor: u64,
    patch: u64,
    pre_release: Vec<PreIdentifier>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreIdentifier {
    Numeric(u64),
    Alphanumeric(String),
}

impl SemanticVersion {
    /// `X.Y.Z[-pre][+build]`；build 元数据不参与比较。
    pub fn parse(s: &str) -> Option<SemanticVersion> {
        let (core_pre, _build) = match s.split_once('+') {
            Some((a, b)) => (a, Some(b)),
            None => (s, None),
        };
        let (core, pre) = match core_pre.split_once('-') {
            Some((a, b)) => (a, Some(b)),
            None => (core_pre, None),
        };
        let mut parts = core.split('.');
        let parse_number = |s: Option<&str>| -> Option<u64> {
            let s = s?;
            if s.is_empty() || (s.len() > 1 && s.starts_with('0')) {
                return None;
            }
            s.parse().ok()
        };
        let major = parse_number(parts.next())?;
        let minor = parse_number(parts.next())?;
        let patch = parse_number(parts.next())?;
        if parts.next().is_some() {
            return None;
        }
        let mut pre_release = Vec::new();
        if let Some(pre) = pre {
            if pre.is_empty() {
                return None;
            }
            for id in pre.split('.') {
                if id.is_empty() {
                    return None;
                }
                if id.bytes().all(|b| b.is_ascii_digit()) {
                    // 数值标识符不允许前导零（除非就是 "0"）
                    if id.len() > 1 && id.starts_with('0') {
                        return None;
                    }
                    pre_release.push(PreIdentifier::Numeric(id.parse().ok()?));
                } else {
                    if !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    {
                        return None;
                    }
                    pre_release.push(PreIdentifier::Alphanumeric(id.to_string()));
                }
            }
        }
        Some(SemanticVersion {
            major,
            minor,
            patch,
            pre_release,
        })
    }
}

impl PartialOrd for SemanticVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemanticVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| match (self.pre_release.is_empty(), other.pre_release.is_empty()) {
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater, // 正式版 > 预发布版
                (false, true) => Ordering::Less,
                (false, false) => self
                    .pre_release
                    .iter()
                    .cmp(other.pre_release.iter()),
            })
    }
}

impl PartialOrd for PreIdentifier {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PreIdentifier {
    fn cmp(&self, other: &Self) -> Ordering {
        use PreIdentifier::*;
        match (self, other) {
            (Numeric(a), Numeric(b)) => a.cmp(b),
            // 数值标识符 < 字母数字标识符
            (Numeric(_), Alphanumeric(_)) => Ordering::Less,
            (Alphanumeric(_), Numeric(_)) => Ordering::Greater,
            (Alphanumeric(a), Alphanumeric(b)) => a.cmp(b),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_order() {
        assert!(SemanticVersion::parse("1.7.3").is_some());
        assert!(SemanticVersion::parse("1.7").is_none());
        assert!(SemanticVersion::parse("1.7.3.4").is_none());
        assert!(SemanticVersion::parse("01.7.3").is_none());
        assert!(SemanticVersion::parse("1.7.3-rc.2").is_some());
        assert!(SemanticVersion::parse("1.7.3-rc.2+build.5").is_some());

        let v = |s: &str| SemanticVersion::parse(s).unwrap();
        assert!(v("1.7.3") > v("1.7.2"));
        assert!(v("1.10.0") > v("1.9.9"));
        assert!(v("2.0.0") > v("1.99.99"));
        assert!(v("1.7.3") > v("1.7.3-rc.1"));
        assert!(v("1.7.3-rc.2") > v("1.7.3-rc.1"));
        assert!(v("1.7.3-rc.2") > v("1.7.3-rc.1"));
        assert!(v("1.7.3-alpha") < v("1.7.3-beta"));
        assert!(v("1.7.3-1") < v("1.7.3-alpha")); // 数值 < 字母数字
        assert_eq!(v("1.7.3"), v("1.7.3+build9")); // build 元数据不参与
    }
}
