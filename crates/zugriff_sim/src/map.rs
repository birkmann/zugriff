//! Valve 220 `.map` files, the format TrenchBroom writes and our layout scripts emit.
//!
//! A map is a list of entities; each has key/value properties and convex brushes. A brush face is three points on its
//! plane (clockwise seen from outside, so the plane normal points out of the brush), a texture name and the Valve 220
//! texture axes. Positions stay in map units (inches, Z up) here; [`crate::units`] converts to the sim's metres/Y up.

use glam::DVec3;

#[derive(Clone, Debug, Default)]
pub struct MapFile {
    pub entities: Vec<MapEntity>,
}

#[derive(Clone, Debug, Default)]
pub struct MapEntity {
    pub props: Vec<(String, String)>,
    pub brushes: Vec<BrushDef>,
}

impl MapEntity {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }

    /// `origin` as map units, if present and well formed.
    pub fn origin(&self) -> Option<DVec3> {
        let v: Vec<f64> = self.get("origin")?.split_whitespace().filter_map(|t| t.parse().ok()).collect();
        (v.len() == 3).then(|| DVec3::new(v[0], v[1], v[2]))
    }
}

#[derive(Clone, Debug, Default)]
pub struct BrushDef {
    pub faces: Vec<FaceDef>,
}

#[derive(Clone, Debug)]
pub struct FaceDef {
    pub points: [DVec3; 3],
    pub texture: String,
    /// Valve 220 axes: `[x y z offset]`. Texel = dot(point, axis) / scale + offset.
    pub u: [f64; 4],
    pub v: [f64; 4],
    pub rotation: f64,
    pub scale: [f64; 2],
}

#[derive(Debug, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "map line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Open,
    Close,
    ParenOpen,
    ParenClose,
    BracketOpen,
    BracketClose,
    Str(String),
    Word(String),
}

fn tokenize(src: &str) -> Vec<(Tok, usize)> {
    let mut out = Vec::new();
    let mut chars = src.chars().peekable();
    let mut line = 1;
    while let Some(&c) = chars.peek() {
        match c {
            '\n' => {
                line += 1;
                chars.next();
            }
            c if c.is_whitespace() => {
                chars.next();
            }
            '/' => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    while let Some(&c) = chars.peek() {
                        if c == '\n' {
                            break;
                        }
                        chars.next();
                    }
                } else {
                    out.push((Tok::Word("/".into()), line));
                }
            }
            '{' | '}' | '(' | ')' | '[' | ']' => {
                chars.next();
                let t = match c {
                    '{' => Tok::Open,
                    '}' => Tok::Close,
                    '(' => Tok::ParenOpen,
                    ')' => Tok::ParenClose,
                    '[' => Tok::BracketOpen,
                    _ => Tok::BracketClose,
                };
                out.push((t, line));
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                for c in chars.by_ref() {
                    if c == '"' {
                        break;
                    }
                    if c == '\n' {
                        line += 1;
                    }
                    s.push(c);
                }
                out.push((Tok::Str(s), line));
            }
            _ => {
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || matches!(c, '{' | '}' | '(' | ')' | '[' | ']' | '"') {
                        break;
                    }
                    s.push(c);
                    chars.next();
                }
                out.push((Tok::Word(s), line));
            }
        }
    }
    out
}

struct Parser {
    toks: Vec<(Tok, usize)>,
    at: usize,
}

impl Parser {
    fn line(&self) -> usize {
        self.toks.get(self.at).or(self.toks.last()).map_or(0, |t| t.1)
    }

    fn err<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError { line: self.line(), message: message.into() })
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at).map(|t| &t.0)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.at).map(|t| t.0.clone());
        self.at += 1;
        t
    }

    fn expect(&mut self, want: Tok) -> Result<(), ParseError> {
        match self.next() {
            Some(t) if t == want => Ok(()),
            other => self.err(format!("expected {want:?}, found {other:?}")),
        }
    }

    fn number(&mut self) -> Result<f64, ParseError> {
        match self.next() {
            Some(Tok::Word(w)) => w.parse().or_else(|_| self.err(format!("not a number: {w}"))),
            other => self.err(format!("expected a number, found {other:?}")),
        }
    }

    fn point(&mut self) -> Result<DVec3, ParseError> {
        self.expect(Tok::ParenOpen)?;
        let p = DVec3::new(self.number()?, self.number()?, self.number()?);
        self.expect(Tok::ParenClose)?;
        Ok(p)
    }

    fn axis(&mut self) -> Result<[f64; 4], ParseError> {
        self.expect(Tok::BracketOpen)?;
        let a = [self.number()?, self.number()?, self.number()?, self.number()?];
        self.expect(Tok::BracketClose)?;
        Ok(a)
    }

    fn face(&mut self) -> Result<FaceDef, ParseError> {
        let points = [self.point()?, self.point()?, self.point()?];
        let texture = match self.next() {
            Some(Tok::Word(w)) | Some(Tok::Str(w)) => w,
            other => return self.err(format!("expected a texture name, found {other:?}")),
        };
        if self.peek() != Some(&Tok::BracketOpen) {
            return self.err("only the Valve 220 format is supported (set the map format to Valve in TrenchBroom)");
        }
        let u = self.axis()?;
        let v = self.axis()?;
        let rotation = self.number()?;
        let scale = [self.number()?, self.number()?];
        Ok(FaceDef { points, texture, u, v, rotation, scale })
    }

    fn brush(&mut self) -> Result<BrushDef, ParseError> {
        let mut faces = Vec::new();
        loop {
            match self.peek() {
                Some(Tok::ParenOpen) => faces.push(self.face()?),
                Some(Tok::Close) => {
                    self.next();
                    return Ok(BrushDef { faces });
                }
                Some(Tok::Word(w)) if w == "patchDef2" || w == "brushDef" => {
                    return self.err(format!("{w} is not supported"));
                }
                other => return self.err(format!("unexpected {other:?} in a brush")),
            }
        }
    }

    fn entity(&mut self) -> Result<MapEntity, ParseError> {
        let mut e = MapEntity::default();
        loop {
            match self.next() {
                Some(Tok::Str(k)) => match self.next() {
                    Some(Tok::Str(v)) => e.props.push((k, v)),
                    other => return self.err(format!("expected a value for \"{k}\", found {other:?}")),
                },
                Some(Tok::Open) => e.brushes.push(self.brush()?),
                Some(Tok::Close) => return Ok(e),
                other => return self.err(format!("unexpected {other:?} in an entity")),
            }
        }
    }
}

/// Parses a Valve 220 `.map` file.
pub fn parse(src: &str) -> Result<MapFile, ParseError> {
    let mut p = Parser { toks: tokenize(src), at: 0 };
    let mut map = MapFile::default();
    while let Some(t) = p.next() {
        if t != Tok::Open {
            return p.err(format!("expected an entity, found {t:?}"));
        }
        map.entities.push(p.entity()?);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const CUBE: &str = r#"// Game: ZUGRIFF
// Format: Valve
{
"classname" "worldspawn"
"mapversion" "220"
// brush 0
{
( -64 -64 -16 ) ( -64 -63 -16 ) ( -64 -64 -15 ) dev/floor [ 0 -1 0 0 ] [ 0 0 -1 0 ] 0 1 1
( -64 -64 -16 ) ( -64 -64 -15 ) ( -63 -64 -16 ) dev/floor [ 1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1
( -64 -64 -16 ) ( -63 -64 -16 ) ( -64 -63 -16 ) dev/floor [ -1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
( 64 64 16 ) ( 64 65 16 ) ( 65 64 16 ) dev/floor [ 1 0 0 0 ] [ 0 -1 0 0 ] 0 1 1
( 64 64 16 ) ( 65 64 16 ) ( 64 64 17 ) dev/floor [ -1 0 0 0 ] [ 0 0 -1 0 ] 0 1 1
( 64 64 16 ) ( 64 64 17 ) ( 64 65 16 ) dev/floor [ 0 1 0 0 ] [ 0 0 -1 0 ] 0 1 1
}
}
{
"classname" "info_player_ksk"
"origin" "0 0 52"
"angle" "90"
}
"#;

    #[test]
    fn parses_entities_and_brushes() {
        let map = parse(CUBE).unwrap();
        assert_eq!(map.entities.len(), 2);
        assert_eq!(map.entities[0].classname(), "worldspawn");
        assert_eq!(map.entities[0].brushes.len(), 1);
        assert_eq!(map.entities[0].brushes[0].faces.len(), 6);
        let f = &map.entities[0].brushes[0].faces[0];
        assert_eq!(f.texture, "dev/floor");
        assert_eq!(f.u, [0.0, -1.0, 0.0, 0.0]);
        assert_eq!(f.scale, [1.0, 1.0]);
        assert_eq!(map.entities[1].origin(), Some(DVec3::new(0.0, 0.0, 52.0)));
    }

    #[test]
    fn rejects_the_standard_format_with_a_hint() {
        let src = "{\n\"classname\" \"worldspawn\"\n{\n( 0 0 0 ) ( 0 1 0 ) ( 1 0 0 ) tex 0 0 0 1 1\n}\n}";
        let e = parse(src).unwrap_err();
        assert_eq!(e.line, 4);
        assert!(e.message.contains("Valve 220"));
    }
}
