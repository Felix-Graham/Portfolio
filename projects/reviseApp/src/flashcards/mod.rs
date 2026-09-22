pub mod card;
pub mod deck;
pub mod import;

pub use card::Flashcard;
pub use deck::Deck;
pub use import::{parse_deck_text, parse_pairs, ImportError};

/// A tiny xorshift64 step — not cryptographic, just enough spread to
/// shuffle a study deck without pulling in the `rand` crate for one
/// button. The caller reseeds it (e.g. from the system clock) each
/// time it wants a fresh shuffle.
fn xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// A seeded Fisher-Yates shuffle of `0..len`, returned as a pure
/// function per this project's convention of testing logic separately
/// from the `egui` code that calls it (see `ui::flashcards`'s
/// "Shuffle" button). Deterministic for a given seed, so it's
/// directly unit-testable without any randomness in the test itself.
pub fn shuffled_order(len: usize, seed: u64) -> Vec<usize> {
    let mut order: Vec<usize> = (0..len).collect();
    let mut state = seed | 1; // xorshift needs a non-zero state
    for i in (1..order.len()).rev() {
        let r = (xorshift(&mut state) as usize) % (i + 1);
        order.swap(i, r);
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shuffled_order_is_a_permutation_of_the_input_range() {
        let mut order = shuffled_order(10, 42);
        order.sort_unstable();
        assert_eq!(order, (0..10).collect::<Vec<_>>());
    }

    #[test]
    fn same_seed_gives_the_same_order() {
        assert_eq!(shuffled_order(20, 7), shuffled_order(20, 7));
    }

    #[test]
    fn different_seeds_usually_give_different_orders() {
        assert_ne!(shuffled_order(20, 7), shuffled_order(20, 99));
    }

    #[test]
    fn handles_zero_and_one_length_without_panicking() {
        assert_eq!(shuffled_order(0, 1), Vec::<usize>::new());
        assert_eq!(shuffled_order(1, 1), vec![0]);
    }
}
