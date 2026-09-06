//! Clean-room resource-pack blockstate/model resolution into reusable quads.

use std::{collections::HashMap, path::Path};

use mc_world::BlockState;

use super::blockstate::{Application, select_applications};

#[derive(Debug, Clone)]
pub(super) struct QuadRef {
    pub positions: [[f32; 3]; 4],
    pub path: String,
    pub uv: [[f32; 2]; 4],
    pub tinted: bool,
    pub cull: Option<Direction>,
    pub brightness: f32,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ModelRefs(pub Vec<QuadRef>);

impl ModelRefs {
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|quad| quad.path.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Direction {
    Up,
    Down,
    North,
    South,
    East,
    West,
}

impl Direction {
    const ALL: [Self; 6] = [Self::Up, Self::Down, Self::North, Self::South, Self::East, Self::West];

    fn parse(value: &str) -> Option<Self> {
        match value {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "north" => Some(Self::North),
            "south" => Some(Self::South),
            "east" => Some(Self::East),
            "west" => Some(Self::West),
            _ => None,
        }
    }

    pub const fn offset(self) -> (i32, i32, i32) {
        match self {
            Self::Up => (0, 1, 0),
            Self::Down => (0, -1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::East => (1, 0, 0),
            Self::West => (-1, 0, 0),
        }
    }

    const fn brightness(self) -> f32 {
        match self {
            Self::Up => 1.0,
            Self::Down => 0.4,
            Self::North | Self::South => 0.8,
            Self::East | Self::West => 0.6,
        }
    }
}

#[derive(Default)]
struct Model {
    textures: HashMap<String, String>,
    elements: Vec<serde_json::Value>,
}

pub(super) fn resolve_block(assets_root: &Path, state: &BlockState) -> Option<ModelRefs> {
    let short = state.name.strip_prefix("minecraft:").unwrap_or(&state.name);
    let blockstate = read_json(&assets_root.join("blockstates").join(format!("{short}.json")))?;
    let applications = select_applications(&blockstate, state)?;
    let mut output = Vec::new();
    for application in applications {
        let model = resolve_model(assets_root, &application.model, 0)?;
        for element in &model.elements {
            bake_element(element, &model.textures, &application, &mut output)?;
        }
    }
    (!output.is_empty()).then_some(ModelRefs(output))
}

fn angle(value: Option<&serde_json::Value>) -> Option<u16> {
    let value = value.map_or(Some(0), serde_json::Value::as_u64)?;
    let value = u16::try_from(value).ok()?;
    (value < 360 && value % 90 == 0).then_some(value)
}

fn resolve_model(assets_root: &Path, reference: &str, depth: usize) -> Option<Model> {
    if depth >= 32 {
        return None;
    }
    let short = reference.strip_prefix("minecraft:").unwrap_or(reference);
    let short = short.strip_prefix("block/").unwrap_or(short);
    let value = read_json(&assets_root.join("models/block").join(format!("{short}.json")))?;
    let mut model = match value.get("parent").and_then(serde_json::Value::as_str) {
        Some(parent) => resolve_model(assets_root, parent, depth + 1)?,
        None => Model::default(),
    };
    if let Some(textures) = value.get("textures").and_then(serde_json::Value::as_object) {
        for (name, value) in textures {
            let texture = value.as_str().or_else(|| value.get("sprite")?.as_str())?;
            model.textures.insert(name.clone(), texture.to_owned());
        }
    }
    if let Some(elements) = value.get("elements").and_then(serde_json::Value::as_array) {
        model.elements.clone_from(elements);
    }
    Some(model)
}

fn bake_element(
    element: &serde_json::Value,
    textures: &HashMap<String, String>,
    application: &Application,
    output: &mut Vec<QuadRef>,
) -> Option<()> {
    let from = vector(element.get("from")?)?;
    let to = vector(element.get("to")?)?;
    let faces = element.get("faces")?.as_object()?;
    let shade = element.get("shade").and_then(serde_json::Value::as_bool).unwrap_or(true);
    for direction in Direction::ALL {
        let Some(face) = faces.get(direction_name(direction)) else { continue };
        let mut positions = face_positions(direction, from, to);
        if let Some(rotation) = element.get("rotation") {
            rotate_element(&mut positions, rotation)?;
        }
        let mut transformed_direction = direction;
        for _ in 0..application.x / 90 {
            positions = positions.map(|point| rotate_point_x(point, [0.5; 3], 90.0, false));
            transformed_direction = rotate_direction_x(transformed_direction);
        }
        for _ in 0..application.y / 90 {
            positions = positions.map(|point| rotate_point_y(point, [0.5; 3], 90.0, false));
            transformed_direction = rotate_direction_y(transformed_direction);
        }
        let uv_rect =
            face.get("uv").map_or_else(|| Some(default_uv(direction, from, to)), vector4)?;
        let turns = angle(face.get("rotation"))? / 90;
        let uv_direction = if application.uvlock { transformed_direction } else { direction };
        let uv = rotate_uv(face_uv(uv_direction, uv_rect), turns);
        let texture = resolve_texture(textures, face.get("texture")?.as_str()?)?;
        let cull = face
            .get("cullface")
            .and_then(serde_json::Value::as_str)
            .and_then(Direction::parse)
            .map(|mut direction| {
                for _ in 0..application.x / 90 {
                    direction = rotate_direction_x(direction);
                }
                for _ in 0..application.y / 90 {
                    direction = rotate_direction_y(direction);
                }
                direction
            });
        output.push(QuadRef {
            positions,
            path: texture,
            uv,
            tinted: face.get("tintindex").is_some(),
            cull,
            brightness: if shade { transformed_direction.brightness() } else { 1.0 },
        });
    }
    Some(())
}

#[allow(clippy::cast_possible_truncation)]
// Resource coordinates are tiny authored decimal values.
fn vector(value: &serde_json::Value) -> Option<[f32; 3]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()? as f32 / 16.0,
        values.get(1)?.as_f64()? as f32 / 16.0,
        values.get(2)?.as_f64()? as f32 / 16.0,
    ])
}

#[allow(clippy::cast_possible_truncation)]
// Resource UV coordinates are conventionally in the 0..16 tile range.
fn vector4(value: &serde_json::Value) -> Option<[f32; 4]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()? as f32,
        values.get(1)?.as_f64()? as f32,
        values.get(2)?.as_f64()? as f32,
        values.get(3)?.as_f64()? as f32,
    ])
}

const fn face_positions(direction: Direction, from: [f32; 3], to: [f32; 3]) -> [[f32; 3]; 4] {
    let [x0, y0, z0] = from;
    let [x1, y1, z1] = to;
    match direction {
        Direction::Up => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
        Direction::Down => [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
        Direction::South => [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
        Direction::North => [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
        Direction::East => [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
        Direction::West => [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
    }
}

const fn default_uv(direction: Direction, from: [f32; 3], to: [f32; 3]) -> [f32; 4] {
    let [x0, y0, z0] = [from[0] * 16.0, from[1] * 16.0, from[2] * 16.0];
    let [x1, y1, z1] = [to[0] * 16.0, to[1] * 16.0, to[2] * 16.0];
    match direction {
        Direction::Down => [x0, 16.0 - z1, x1, 16.0 - z0],
        Direction::Up => [x0, z0, x1, z1],
        Direction::North => [16.0 - x1, 16.0 - y1, 16.0 - x0, 16.0 - y0],
        Direction::South => [x0, 16.0 - y1, x1, 16.0 - y0],
        Direction::West => [z0, 16.0 - y1, z1, 16.0 - y0],
        Direction::East => [16.0 - z1, 16.0 - y1, 16.0 - z0, 16.0 - y0],
    }
}

const fn face_uv(direction: Direction, [u0, v0, u1, v1]: [f32; 4]) -> [[f32; 2]; 4] {
    let standard = [[u0, v1], [u1, v1], [u1, v0], [u0, v0]];
    match direction {
        Direction::Up | Direction::Down | Direction::South => standard,
        _ => [[u1, v1], [u0, v1], [u0, v0], [u1, v0]],
    }
}

fn rotate_uv(mut uv: [[f32; 2]; 4], turns: u16) -> [[f32; 2]; 4] {
    for _ in 0..turns {
        uv.rotate_right(1);
    }
    uv
}

#[allow(clippy::cast_possible_truncation)]
// The resource format restricts element angles to small fixed values.
fn rotate_element(points: &mut [[f32; 3]; 4], value: &serde_json::Value) -> Option<()> {
    let origin = vector(value.get("origin")?)?;
    let angle = value.get("angle")?.as_f64()? as f32;
    let rescale = value.get("rescale").and_then(serde_json::Value::as_bool).unwrap_or(false);
    let axis = value.get("axis")?.as_str()?;
    *points = points.map(|point| match axis {
        "x" => rotate_point_x(point, origin, angle, rescale),
        "y" => rotate_point_y(point, origin, angle, rescale),
        "z" => rotate_point_z(point, origin, angle, rescale),
        _ => point,
    });
    Some(())
}

#[allow(clippy::suboptimal_flops)] // Runs only during one-time model baking.
fn rotate_point_x(mut point: [f32; 3], origin: [f32; 3], degrees: f32, rescale: bool) -> [f32; 3] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (y, z) = (point[1] - origin[1], point[2] - origin[2]);
    let scale = if rescale { 1.0 / cos.abs() } else { 1.0 };
    point[1] = origin[1] + (y * cos - z * sin) * scale;
    point[2] = origin[2] + (y * sin + z * cos) * scale;
    point
}

#[allow(clippy::suboptimal_flops)] // Runs only during one-time model baking.
fn rotate_point_y(mut point: [f32; 3], origin: [f32; 3], degrees: f32, rescale: bool) -> [f32; 3] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (x, z) = (point[0] - origin[0], point[2] - origin[2]);
    let scale = if rescale { 1.0 / cos.abs() } else { 1.0 };
    point[0] = origin[0] + (x * cos - z * sin) * scale;
    point[2] = origin[2] + (x * sin + z * cos) * scale;
    point
}

#[allow(clippy::suboptimal_flops)] // Runs only during one-time model baking.
fn rotate_point_z(mut point: [f32; 3], origin: [f32; 3], degrees: f32, rescale: bool) -> [f32; 3] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let (x, y) = (point[0] - origin[0], point[1] - origin[1]);
    let scale = if rescale { 1.0 / cos.abs() } else { 1.0 };
    point[0] = origin[0] + (x * cos - y * sin) * scale;
    point[1] = origin[1] + (x * sin + y * cos) * scale;
    point
}

const fn rotate_direction_x(direction: Direction) -> Direction {
    match direction {
        Direction::Up => Direction::South,
        Direction::South => Direction::Down,
        Direction::Down => Direction::North,
        Direction::North => Direction::Up,
        other => other,
    }
}

const fn rotate_direction_y(direction: Direction) -> Direction {
    match direction {
        Direction::North => Direction::West,
        Direction::West => Direction::South,
        Direction::South => Direction::East,
        Direction::East => Direction::North,
        other => other,
    }
}

const fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Up => "up",
        Direction::Down => "down",
        Direction::North => "north",
        Direction::South => "south",
        Direction::East => "east",
        Direction::West => "west",
    }
}

fn resolve_texture(textures: &HashMap<String, String>, texture: &str) -> Option<String> {
    let mut current = texture;
    for _ in 0..32 {
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

#[cfg(test)]
mod tests {
    use super::{Direction, default_uv};

    #[test]
    #[allow(clippy::float_cmp)] // Values are exactly representable binary fractions.
    fn default_uv_uses_element_bounds() {
        assert_eq!(
            default_uv(Direction::South, [0.25, 0.5, 0.0], [0.75, 1.0, 1.0]),
            [4.0, 0.0, 12.0, 8.0]
        );
    }
}
