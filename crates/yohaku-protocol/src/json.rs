//! JSON 线格式约定：
//! 1. 所有键排序输出（macOS 版 JSONEncoder.sortedKeys 的等价物）；
//! 2. 「键存在但值为 null」与「键缺失」是两种语义——
//!    - 显式 null：字段直接用 `Option<T>`（无 skip 属性）；
//!    - 可缺失键：字段用 `MaybeAbsent<T>` 或 `#[serde(skip_serializing_if = "Option::is_none")]`；
//!    - 要求键必须存在但可为 null（解码侧）：字段用 `RequiredNullable<T>`。
//!
//! 注意：serde_json 未启用 `preserve_order`，`Value::Object` 即 BTreeMap，
//! 经 Value 往返即可得到全层级排序键。

use serde::{Deserialize, Deserializer, Serialize};

pub fn to_sorted_json<T: Serialize>(v: &T) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(v)?;
    serde_json::to_string(&value)
}

/// 解码侧：「键必须存在，值可为 null」。字段类型为本包装而非 `Option`，
/// serde 因此在键缺失时报 missing-field 错误而不是默认 None。
#[derive(Debug, Clone, PartialEq)]
pub struct RequiredNullable<T>(pub Option<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for RequiredNullable<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(Option::<T>::deserialize(d)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;

    #[derive(Serialize)]
    struct ExplicitNull {
        v: Option<u32>,
    }

    #[derive(Serialize)]
    struct AbsentAble {
        #[serde(skip_serializing_if = "Option::is_none")]
        v: Option<u32>,
    }

    #[test]
    fn explicit_null_is_emitted() {
        let s = to_sorted_json(&ExplicitNull { v: None }).unwrap();
        assert_eq!(s, r#"{"v":null}"#);
        let s = to_sorted_json(&ExplicitNull { v: Some(3) }).unwrap();
        assert_eq!(s, r#"{"v":3}"#);
    }

    #[test]
    fn absent_key_is_omitted() {
        let s = to_sorted_json(&AbsentAble { v: None }).unwrap();
        assert_eq!(s, "{}");
    }

    #[test]
    fn sorted_keys_at_all_levels() {
        let s = to_sorted_json(&serde_json::json!({"b":1,"a":{"d":2,"c":null}})).unwrap();
        assert_eq!(s, r#"{"a":{"c":null,"d":2},"b":1}"#);
    }

    #[derive(Deserialize)]
    struct Required {
        v: RequiredNullable<u32>,
    }

    #[test]
    fn required_nullable_missing_key_fails_but_null_ok() {
        assert!(serde_json::from_str::<Required>(r#"{}"#).is_err());
        let r: Required = serde_json::from_str(r#"{"v":null}"#).unwrap();
        assert_eq!(r.v.0, None);
        let r: Required = serde_json::from_str(r#"{"v":7}"#).unwrap();
        assert_eq!(r.v.0, Some(7));
    }
}
