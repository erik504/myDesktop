pub fn from_count(count: u32) -> Option<String> {
    if count > 999_999 { return None; }
    Some(format!("{}.{}.{}", count / 10_000, count / 100 % 100, count % 100))
}

pub fn validate(value: &str) -> Option<String> {
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 3 { return None; }
    let nums: Vec<u32> = parts.iter().map(|s| {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) { return None; }
        s.parse::<u32>().ok().filter(|n| *n <= 99)
    }).collect::<Option<_>>()?;
    Some(format!("{}.{}.{}", nums[0], nums[1], nums[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn base_100_carries() {
        for (count, value) in [(1,"0.0.1"), (98,"0.0.98"), (99,"0.0.99"), (100,"0.1.0"), (101,"0.1.1"), (9999,"0.99.99"), (10000,"1.0.0"), (999999,"99.99.99")] {
            assert_eq!(from_count(count).as_deref(), Some(value));
        }
        assert!(from_count(1_000_000).is_none());
    }
    #[test] fn explicit_version_validation() {
        assert_eq!(validate("00.01.99").as_deref(), Some("0.1.99"));
        for invalid in ["1.2", "1.2.100", "1.-1.0", "x.0.1", "1.2.3.4"] { assert!(validate(invalid).is_none()); }
    }
}
