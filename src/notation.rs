//! Lecture et écriture de la notation d'une partie.

use crate::game::{BLUE, Move, Position, Refusal, WHITE};
use crate::pieces::{SHAPE_NAMES, orientation_index, orientations};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordError {
    /// Rang du coup fautif, à partir de 1, hors indication de premier joueur.
    pub index: usize,
    pub token: String,
    pub reason: Refusal,
}

pub fn parse_move(token: &str) -> Result<Move, Refusal> {
    let t = token.trim().to_ascii_lowercase();
    let b = t.as_bytes();
    let (shape, mut i) = match b {
        [b'1', ..] => (0, 1),
        [b'2', ..] => (1, 1),
        [b'3', b'i', ..] => (2, 2),
        [b'3', b'l', ..] => (3, 2),
        [b'4', b's', ..] => (4, 2),
        [b'4', b't', ..] => (5, 2),
        [b'4', b'l', ..] => (6, 2),
        _ => return Err(Refusal::Syntax),
    };
    let mut flipped = false;
    if b.get(i) == Some(&b's') {
        flipped = true;
        i += 1;
    }
    let mut rotation = 0u8;
    if let Some(&d) = b.get(i).filter(|&&c| c == b'r' || c == b'l') {
        let k = match b.get(i + 1) {
            Some(c @ b'1'..=b'3') => c - b'0',
            _ => return Err(Refusal::Syntax),
        };
        rotation = if d == b'r' { k } else { 4 - k };
        i += 2;
    }
    let col = match b.get(i) {
        Some(c @ b'1'..=b'9') if i + 1 == b.len() => c - b'1',
        _ => return Err(Refusal::Syntax),
    };
    Ok(Move { shape, orient: orientation_index(shape as usize, rotation, flipped) as u8, col })
}

pub fn format_move(m: Move) -> String {
    let o = &orientations(m.shape as usize)[m.orient as usize];
    let mut s = SHAPE_NAMES[m.shape as usize].to_string();
    if o.flipped {
        s.push('s');
    }
    if o.rotation > 0 {
        s.push('r');
        s.push((b'0' + o.rotation) as char);
    }
    s.push((b'1' + m.col) as char);
    s
}

/// Rejoue une notation depuis une partie vierge.
pub fn parse_record(source: &str) -> Result<Position, RecordError> {
    let mut tokens = source
        .split(|c: char| c.is_whitespace() || c == ',' || c == '+')
        .filter(|t| !t.is_empty())
        .peekable();
    let first = match tokens.peek().map(|t| t.to_ascii_lowercase()) {
        Some(t) if t == "b" || t == "blue" => Some(BLUE),
        Some(t) if t == "w" || t == "white" => Some(WHITE),
        _ => None,
    };
    if first.is_some() {
        tokens.next();
    }
    let mut pos = Position::new(first.unwrap_or(BLUE));
    let mut pending_pass = false;
    for (n, token) in tokens.enumerate() {
        let err = |reason| RecordError { index: n + 1, token: token.to_string(), reason };
        if token == "--" {
            if !pending_pass || pos.outcome.is_some() {
                return Err(err(Refusal::UnexpectedPass));
            }
            pending_pass = false;
            continue;
        }
        let m = parse_move(token).map_err(err)?;
        let mover = pos.to_move;
        pos.play(m).map_err(err)?;
        pending_pass = pos.outcome.is_none() && pos.to_move == mover;
    }
    Ok(pos)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Outcome;

    #[test]
    fn canonical_table() {
        let expected = [
            vec!["11"],
            vec!["21", "2r11"],
            vec!["3I1", "3Ir11"],
            vec!["3L1", "3Lr11", "3Lr21", "3Lr31"],
            vec!["4S1", "4Sr11", "4Ss1", "4Ssr11"],
            vec!["4T1", "4Tr11", "4Tr21", "4Tr31"],
            vec!["4L1", "4Lr11", "4Lr21", "4Lr31", "4Ls1", "4Lsr11", "4Lsr21", "4Lsr31"],
        ];
        for (shape, tokens) in expected.iter().enumerate() {
            let got: Vec<String> = (0..orientations(shape).len())
                .map(|o| format_move(Move { shape: shape as u8, orient: o as u8, col: 0 }))
                .collect();
            assert_eq!(&got, tokens);
        }
    }

    #[test]
    fn normalization_examples() {
        for (input, canonical) in [
            ("2r23", "23"),
            ("1r27", "17"),
            ("2l13", "2r13"),
            ("3Ls4", "3Lr14"),
            ("4Ls5", "4Ls5"),
            ("4Ssr21", "4Ss1"),
            ("4lsr27", "4Lsr27"),
            (" 3Ir13 ", "3Ir13"),
        ] {
            assert_eq!(format_move(parse_move(input).unwrap()), canonical, "{input}");
        }
    }

    #[test]
    fn syntax_errors() {
        for bad in ["", "--", "10", "5", "3x4", "4L", "4Lr4", "4Lr02", "15 16", "4Lr1", "1r", "4L10"] {
            assert_eq!(parse_move(bad), Err(Refusal::Syntax), "{bad}");
        }
    }

    #[test]
    fn record_replay_and_first_player() {
        let pos = parse_record("w 15 3Ir13").unwrap();
        assert_eq!(pos.to_move, WHITE);
        assert_eq!(pos.heights[4], 1);
        assert_eq!(pos.heights[2], 3);
        assert_eq!(pos.inv[WHITE][0], 1);
        assert_eq!(pos.inv[BLUE][2], 1);
        assert_eq!(parse_record("").unwrap().to_move, BLUE);
        assert_eq!(parse_record("b 15+16,17").unwrap().to_move, WHITE);
    }

    #[test]
    fn record_errors() {
        let e = parse_record("15 15 15 15 15").unwrap_err();
        assert_eq!((e.index, e.reason), (5, Refusal::Exhausted));
        let e = parse_record("15 --").unwrap_err();
        assert_eq!((e.index, e.reason), (2, Refusal::UnexpectedPass));
        let e = parse_record("4T1").unwrap_err();
        assert_eq!(e.reason, Refusal::Unsupported);
    }

    #[test]
    fn horizontal_win_along_floor() {
        let pos = parse_record("3I1 11 3I4 12 4Lr27").unwrap();
        assert_eq!(pos.outcome, Some(Outcome::Connection(BLUE)));
        let e = parse_record("3I1 11 3I4 12 4Lr27 17").unwrap_err();
        assert_eq!(e.reason, Refusal::Finished);
    }
}
