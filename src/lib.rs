// Rust 1.100 : clippy signale les boucles indexées des tables du jeu (zobrist,
// hauteurs, inventaires). Elles parcourent plusieurs tableaux par le même indice
// et restent telles quelles : pas de réécriture du moteur pour un lint de style.
#![allow(clippy::needless_range_loop)]

pub mod book;
pub mod eval;
pub mod game;
pub mod notation;
pub mod pieces;
pub mod search;
