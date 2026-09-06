//! Selection rules for resource-pack blockstate variants and multipart models.

use mc_world::BlockState;

#[derive(Debug, Clone)]
pub(super) struct Application {
    pub model: String,
    pub x: u16,
    pub y: u16,
    pub uvlock: bool,
}

pub(super) fn select_applications(
    value: &serde_json::Value,
    state: &BlockState,
) -> Option<Vec<Application>> {
    if let Some(variants) = value.get("variants").and_then(serde_json::Value::as_object) {
        let selected = variants.iter().find(|(key, _)| properties_match(key, state))?.1;
        return Some(vec![parse_application(first_entry(selected))?]);
    }
    let parts = value.get("multipart")?.as_array()?;
    let selected = parts
        .iter()
        .filter(|part| part.get("when").is_none_or(|when| condition_matches(when, state)))
        .filter_map(|part| part.get("apply"))
        .map(|apply| parse_application(first_entry(apply)))
        .collect::<Option<Vec<_>>>()?;
    (!selected.is_empty()).then_some(selected)
}

fn first_entry(value: &serde_json::Value) -> &serde_json::Value {
    value.as_array().and_then(|entries| entries.first()).unwrap_or(value)
}

fn parse_application(value: &serde_json::Value) -> Option<Application> {
    Some(Application {
        model: value.get("model")?.as_str()?.to_owned(),
        x: angle(value.get("x"))?,
        y: angle(value.get("y"))?,
        uvlock: value.get("uvlock").and_then(serde_json::Value::as_bool).unwrap_or(false),
    })
}

fn properties_match(key: &str, state: &BlockState) -> bool {
    key.is_empty()
        || key.split(',').all(|part| {
            part.split_once('=')
                .is_some_and(|(name, expected)| property_matches(name, expected, state))
        })
}

fn property_matches(name: &str, expected: &str, state: &BlockState) -> bool {
    state
        .properties
        .get(name)
        .is_some_and(|actual| expected.split('|').any(|candidate| candidate == actual.as_ref()))
}

fn condition_matches(value: &serde_json::Value, state: &BlockState) -> bool {
    let Some(object) = value.as_object() else { return false };
    if let Some(conditions) = object.get("OR").and_then(serde_json::Value::as_array) {
        return conditions.iter().any(|condition| condition_matches(condition, state));
    }
    if let Some(conditions) = object.get("AND").and_then(serde_json::Value::as_array) {
        return conditions.iter().all(|condition| condition_matches(condition, state));
    }
    object.iter().all(|(name, expected)| {
        expected.as_str().is_some_and(|expected| property_matches(name, expected, state))
    })
}

fn angle(value: Option<&serde_json::Value>) -> Option<u16> {
    let value = value.map_or(Some(0), serde_json::Value::as_u64)?;
    let value = u16::try_from(value).ok()?;
    (value < 360 && value % 90 == 0).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::{condition_matches, select_applications};
    use mc_world::BlockState;
    use std::collections::BTreeMap;

    #[test]
    fn variant_and_multipart_conditions_match_cached_properties() {
        let state = BlockState {
            name: "minecraft:test".into(),
            properties: BTreeMap::from([
                ("facing".into(), "east".into()),
                ("open".into(), "true".into()),
            ]),
        };
        let variants: serde_json::Value = serde_json::from_str(
            r#"{"variants":{"facing=east,open=true":{"model":"block/open","y":90}}}"#,
        )
        .unwrap();
        let selected = select_applications(&variants, &state).unwrap();
        assert_eq!(selected[0].model, "block/open");
        assert_eq!(selected[0].y, 90);
        let condition: serde_json::Value = serde_json::from_str(
            r#"{"OR":[{"facing":"west"},{"facing":"east","open":"true|false"}]}"#,
        )
        .unwrap();
        assert!(condition_matches(&condition, &state));
    }
}
