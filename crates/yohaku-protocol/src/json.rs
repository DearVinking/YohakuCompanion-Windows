//! JSON 线格式约定：
//! 1. 所有键排序输出（macOS 版 JSONEncoder.sortedKeys 的等价物）；
//! 2. 「键存在但值为 null」与「键缺失」是两种语义——
//!    - 编码侧：显式 null 用 `Option<T>`（无 skip 属性）；可缺失键用
//!      `#[serde(skip_serializing_if = "Option::is_none")]`；
//!    - 解码侧：serde 的字段级反序列化无法区分缺失与 null（missing_field
//!      会经 `deserialize_option → visit_none` 提供默认值），因此响应解码
//!      一律经 [`take_required`] / [`take_nullable`] / [`take_optional`]
//!      在 map 层显式判定，与 macOS 版手写 Codable 语义一致。
//!
//! 注意：serde_json 未启用 `preserve_order`，`Value::Object` 即 BTreeMap，
//! 经 Value 往返即可得到全层级排序键。

use serde::Serialize;
use serde::de::DeserializeOwned;

pub fn to_sorted_json<T: Serialize>(v: &T) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(v)?;
    serde_json::to_string(&value)
}

/// 键必须存在且非 null。
pub fn take_required<T: DeserializeOwned>(
    map: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<T, String> {
    match map.get(key) {
        None => Err(format!("missing key `{key}`")),
        Some(serde_json::Value::Null) => Err(format!("key `{key}` must not be null")),
        Some(v) => T::deserialize(v).map_err(|e| format!("key `{key}`: {e}")),
    }
}

/// 键必须存在，但值可为 null（`decodeRequiredNullable` 语义）。
pub fn take_nullable<T: DeserializeOwned>(
    map: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<T>, String> {
    match map.get(key) {
        None => Err(format!("missing key `{key}`")),
        Some(serde_json::Value::Null) => Ok(None),
        Some(v) => Ok(Some(
            T::deserialize(v).map_err(|e| format!("key `{key}`: {e}"))?,
        )),
    }
}

/// 键可缺失；缺失与 null 均为 None。
pub fn take_optional<T: DeserializeOwned>(
    map: &serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<Option<T>, String> {
    match map.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(v) => Ok(Some(
            T::deserialize(v).map_err(|e| format!("key `{key}`: {e}"))?,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

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

    #[derive(Deserialize, PartialEq, Debug)]
    struct Inner {
        x: u32,
    }

    #[test]
    fn take_distinguishes_missing_null_and_value() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"req":7,"nullable":null,"inner":{"x":1},"opt":null}"#)
                .unwrap();
        let map = value.as_object().unwrap();

        assert_eq!(take_required::<u32>(map, "req").unwrap(), 7);
        assert!(take_required::<u32>(map, "absent").is_err());
        assert!(take_required::<u32>(map, "nullable").is_err());

        assert_eq!(take_nullable::<u32>(map, "nullable").unwrap(), None);
        assert_eq!(take_nullable::<u32>(map, "req").unwrap(), Some(7));
        assert!(take_nullable::<u32>(map, "absent").is_err());

        assert_eq!(take_optional::<u32>(map, "absent").unwrap(), None);
        assert_eq!(take_optional::<u32>(map, "nullable").unwrap(), None);
        assert_eq!(take_optional::<u32>(map, "req").unwrap(), Some(7));

        assert_eq!(
            take_nullable::<Inner>(map, "inner").unwrap(),
            Some(Inner { x: 1 })
        );
    }
}
