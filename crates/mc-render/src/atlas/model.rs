//! Resource-pack blockstate/model resolution for orthogonal full cubes.

use std::{collections::HashMap, path::Path};

use mc_world::BlockState;

#[derive(Debug, Clone)]
pub(super) struct FaceRef {
    pub path: String,
    pub uv: [[f32; 2]; 4],
    pub tinted: bool,
}

#[derive(Debug, Clone)]
pub(super) struct FaceRefs(pub [FaceRef; 6]);

impl FaceRefs {
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|face| face.path.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    North,
    South,
    East,
    West,
}

impl Direction {
    const ALL: [Self; 6] = [Self::Up, Self::Down, Self::North, Self::South, Self::East, Self::West];

    const fn index(self) -> usize {
        match self {
            Self::Up => 0,
            Self::Down => 1,
            Self::North => 2,
            Self::South => 3,
            Self::East => 4,
            Self::West => 5,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::North => "north",
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
        }
    }
}

#[derive(Debug, Clone)]
struct ModelFace {
    texture: String,
    rotation: u16,
    tinted: bool,
}

struct Model {
    textures: HashMap<String, String>,
    faces: Option<[ModelFace; 6]>,
}

struct Variant<'a> {
    model: &'a str,
    x: u16,
    y: u16,
}

pub(super) fn resolve_block(assets_root: &Path, state: &BlockState) -> Option<FaceRefs> {
    let short = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
    let blockstate = read_json(&assets_root.join("blockstates").join(format!("{short}.json")))?;
    let variant = select_variant(&blockstate, state)?;
    let model = resolve_model(assets_root, variant.model)?;
    let model_faces = model.faces?;
    let mut faces = Vec::with_capacity(6);
    for (direction, face) in Direction::ALL.into_iter().zip(model_faces) {
        let path = resolve_texture(&model.textures, &face.texture)?;
        faces.push(FaceRef {
            path,
            uv: rotate_uv(default_uv(direction), face.rotation / 90),
            tinted: face.tinted,
        });
    }
    let faces: [FaceRef; 6] = faces.try_into().ok()?;
    rotate_faces(FaceRefs(faces), variant.x / 90, variant.y / 90)
}

fn select_variant<'a>(
    blockstate: &'a serde_json::Value,
    state: &BlockState,
) -> Option<Variant<'a>> {
    let variants = blockstate.get("variants")?.as_object()?;
    let value =
        variants.iter().find(|(key, _)| variant_matches(key, state)).map(|(_, value)| value)?;
    let entry = value.as_array().and_then(|list| list.first()).unwrap_or(value);
    let x = angle(entry.get("x"))?;
    let y = angle(entry.get("y"))?;
    Some(Variant { model: entry.get("model")?.as_str()?, x, y })
}

fn variant_matches(key: &str, state: &BlockState) -> bool {
    key.is_empty()
        || key.split(',').all(|condition| {
            condition.split_once('=').is_some_and(|(name, expected)| {
                state
                    .properties
                    .get(name)
                    .is_some_and(|actual| expected.split('|').any(|value| value == actual.as_ref()))
            })
        })
}

fn angle(value: Option<&serde_json::Value>) -> Option<u16> {
    let angle = value.map_or(Some(0), serde_json::Value::as_u64)?;
    let angle = u16::try_from(angle).ok()?;
    (angle < 360 && angle % 90 == 0).then_some(angle)
}

fn resolve_model(assets_root: &Path, model_ref: &str) -> Option<Model> {
    let short = model_ref.strip_prefix("minecraft:").unwrap_or(model_ref);
    let short = short.strip_prefix("block/").unwrap_or(short);
    let value = read_json(&assets_root.join("models/block").join(format!("{short}.json")))?;
    let mut model = match value.get("parent").and_then(serde_json::Value::as_str) {
        Some(parent) => resolve_model(assets_root, parent)?,
        None => Model { textures: HashMap::new(), faces: None },
    };
    if let Some(textures) = value.get("textures").and_then(serde_json::Value::as_object) {
        for (key, value) in textures {
            model.textures.insert(key.clone(), value.as_str()?.to_owned());
        }
    }
    if value.get("elements").is_some() {
        model.faces = parse_full_cube(&value);
    }
    Some(model)
}

fn parse_full_cube(model: &serde_json::Value) -> Option<[ModelFace; 6]> {
    let elements = model.get("elements")?.as_array()?;
    let [element] = elements.as_slice() else { return None };
    if element.get("from")?.as_array()? != &[0, 0, 0]
        || element.get("to")?.as_array()? != &[16, 16, 16]
    {
        return None;
    }
    let faces = element.get("faces")?.as_object()?;
    let parsed: Vec<_> = Direction::ALL
        .iter()
        .map(|direction| {
            let face = faces.get(direction.name())?;
            Some(ModelFace {
                texture: face.get("texture")?.as_str()?.to_owned(),
                rotation: angle(face.get("rotation"))?,
                tinted: face.get("tintindex").is_some(),
            })
        })
        .collect::<Option<_>>()?;
    parsed.try_into().ok()
}

fn resolve_texture(textures: &HashMap<String, String>, texture: &str) -> Option<String> {
    let mut current = texture;
    for _ in 0..8 {
        match current.strip_prefix('#') {
            Some(slot) => current = textures.get(slot)?,
            None => return Some(current.to_owned()),
        }
    }
    None
}

fn read_json(path: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

const fn default_uv(direction: Direction) -> [[f32; 2]; 4] {
    match direction {
        Direction::Up | Direction::Down | Direction::South => {
            [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]
        }
        Direction::North | Direction::East | Direction::West => {
            [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]]
        }
    }
}

fn rotate_uv(mut uv: [[f32; 2]; 4], turns: u16) -> [[f32; 2]; 4] {
    for _ in 0..turns {
        for point in &mut uv {
            *point = [1.0 - point[1], point[0]];
        }
    }
    uv
}

fn rotate_faces(mut faces: FaceRefs, x_turns: u16, y_turns: u16) -> Option<FaceRefs> {
    for _ in 0..x_turns {
        faces = rotate_once(&faces, rotate_x)?;
    }
    for _ in 0..y_turns {
        faces = rotate_once(&faces, rotate_y)?;
    }
    Some(faces)
}

fn rotate_once(faces: &FaceRefs, rotate: fn([i8; 3]) -> [i8; 3]) -> Option<FaceRefs> {
    let mut output: [Option<FaceRef>; 6] = std::array::from_fn(|_| None);
    for direction in Direction::ALL {
        let source = &faces.0[direction.index()];
        let rotated_direction = direction_of(rotate(normal(direction)))?;
        let target_corners = geometry_corners(rotated_direction);
        let mut uv = [[0.0; 2]; 4];
        for (corner, source_uv) in geometry_corners(direction).into_iter().zip(source.uv) {
            let rotated = rotate(corner);
            let index = target_corners.iter().position(|candidate| *candidate == rotated)?;
            uv[index] = source_uv;
        }
        output[rotated_direction.index()] =
            Some(FaceRef { path: source.path.clone(), uv, tinted: source.tinted });
    }
    Some(FaceRefs(output.map(|face| face.expect("cube rotation preserves all six faces"))))
}

const fn normal(direction: Direction) -> [i8; 3] {
    match direction {
        Direction::Up => [0, 1, 0],
        Direction::Down => [0, -1, 0],
        Direction::North => [0, 0, -1],
        Direction::South => [0, 0, 1],
        Direction::East => [1, 0, 0],
        Direction::West => [-1, 0, 0],
    }
}

const fn direction_of(normal: [i8; 3]) -> Option<Direction> {
    match normal {
        [0, 1, 0] => Some(Direction::Up),
        [0, -1, 0] => Some(Direction::Down),
        [0, 0, -1] => Some(Direction::North),
        [0, 0, 1] => Some(Direction::South),
        [1, 0, 0] => Some(Direction::East),
        [-1, 0, 0] => Some(Direction::West),
        _ => None,
    }
}

const fn rotate_x([x, y, z]: [i8; 3]) -> [i8; 3] {
    [x, -z, y]
}

const fn rotate_y([x, y, z]: [i8; 3]) -> [i8; 3] {
    [-z, y, x]
}

const fn geometry_corners(direction: Direction) -> [[i8; 3]; 4] {
    match direction {
        Direction::Up => [[-1, 1, -1], [-1, 1, 1], [1, 1, 1], [1, 1, -1]],
        Direction::Down => [[-1, -1, -1], [1, -1, -1], [1, -1, 1], [-1, -1, 1]],
        Direction::South => [[-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]],
        Direction::North => [[-1, -1, -1], [-1, 1, -1], [1, 1, -1], [1, -1, -1]],
        Direction::East => [[1, -1, -1], [1, 1, -1], [1, 1, 1], [1, -1, 1]],
        Direction::West => [[-1, -1, -1], [-1, -1, 1], [-1, 1, 1], [-1, 1, -1]],
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mc_world::BlockState;

    use super::{Direction, FaceRef, FaceRefs, rotate_faces, select_variant};

    #[test]
    fn state_properties_select_the_matching_variant_and_rotation() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"variants":{"axis=x":{"model":"block/log_horizontal","x":90,"y":90},"axis=y":{"model":"block/log"}}}"#,
        )
        .unwrap();
        let state = BlockState {
            name: "minecraft:birch_log".into(),
            properties: BTreeMap::from([("axis".into(), "x".into())]),
        };
        let variant = select_variant(&json, &state).unwrap();
        assert_eq!(variant.model, "block/log_horizontal");
        assert_eq!((variant.x, variant.y), (90, 90));
    }

    #[test]
    fn orthogonal_rotation_moves_end_textures_and_their_uvs() {
        let side = || FaceRef {
            path: "side".to_owned(),
            uv: [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]],
            tinted: false,
        };
        let end = || FaceRef { path: "end".to_owned(), ..side() };
        let faces = FaceRefs([end(), end(), side(), side(), side(), side()]);
        let rotated = rotate_faces(faces, 1, 1).unwrap();
        assert_eq!(rotated.0[Direction::East.index()].path, "end");
        assert_eq!(rotated.0[Direction::West.index()].path, "end");
        assert_ne!(rotated.0[Direction::Up.index()].uv, side().uv);
    }
}
