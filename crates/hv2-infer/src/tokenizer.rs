//! Byte-level BPE, as the model file carries it.
//!
//! A tokeniser is not a detail a forward pass can be demonstrated without. The
//! model has no notion of text: it takes an integer and produces a distribution
//! over integers, and every claim about what it "said" is a claim about this
//! file being right. A tokeniser that is subtly wrong produces a model that
//! answers *almost* sensibly, which is the failure that looks most like success.
//!
//! # Byte-level, and why the vocabulary looks like that
//!
//! Every byte is first mapped to a printable character, so that a vocabulary is
//! a set of ordinary strings and no entry is ever an unprintable byte or a
//! newline. That is why a space appears as `Ġ` and a newline as `Ċ` in the
//! token list — those are the substitutes, not the model being strange. The
//! same table run backwards turns generated tokens back into bytes.
//!
//! # The pre-tokeniser
//!
//! Before any merging, text is cut into pieces that merges may not cross —
//! otherwise `the cat` could merge across the space and the model would see a
//! token it was never trained on. The reference implementation expresses the
//! cuts as one regular expression with a lookahead in it; the rules are
//! implemented directly here instead, because the `regex` crate has no
//! lookahead and pulling in one that does, for six rules, is a poor trade.
//!
//! What that costs is stated rather than hidden: this handles the cases an
//! English prompt is made of, and a language whose words are not separated by
//! spaces would be cut differently here than by the reference.

use std::collections::HashMap;

/// A vocabulary and its merge rules.
pub struct Tokenizer {
    /// Token string to id.
    ids: HashMap<String, u32>,
    /// Id to token string, for decoding.
    tokens: Vec<String>,
    /// A merge rule's rank. Lower is applied first, which is what makes BPE
    /// deterministic.
    merges: HashMap<(String, String), u32>,
    /// Byte value to its printable stand-in, and back.
    to_char: [char; 256],
    from_char: HashMap<char, u8>,
}

impl Tokenizer {
    /// Build from the token list and merge list a GGUF carries.
    pub fn new(tokens: &[String], merges: &[String]) -> Self {
        let mut ids = HashMap::with_capacity(tokens.len());
        for (id, token) in tokens.iter().enumerate() {
            // First writer wins. A vocabulary should have no duplicates, and if
            // it has one the lower id is the one the model was trained to emit.
            ids.entry(token.clone()).or_insert(id as u32);
        }

        let mut rules = HashMap::with_capacity(merges.len());
        for (rank, rule) in merges.iter().enumerate() {
            if let Some((left, right)) = rule.split_once(' ') {
                rules.insert((left.to_string(), right.to_string()), rank as u32);
            }
        }

        let (to_char, from_char) = byte_char_tables();

        Self {
            ids,
            tokens: tokens.to_vec(),
            merges: rules,
            to_char,
            from_char,
        }
    }

    /// How many tokens the vocabulary has.
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Whether it has none.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// The id of a token named exactly `token`, if there is one.
    ///
    /// This is how the special tokens are found — `<|begin_of_text|>` and the
    /// rest — rather than by trusting the ids in the file's metadata. The model
    /// used here carries `bos_token_id = 1`, which is not where its beginning
    /// marker actually is; a name is checkable and a number is not.
    pub fn id_of(&self, token: &str) -> Option<u32> {
        self.ids.get(token).copied()
    }

    /// Encode `text` into token ids.
    ///
    /// Special tokens are not recognised in the text: whatever is here is
    /// treated as ordinary characters. Control tokens are added by the caller,
    /// by id, which is the only way to be sure a prompt cannot smuggle one in.
    pub fn encode(&self, text: &str) -> Vec<u32> {
        let mut out = Vec::new();
        for piece in pre_tokenize(text) {
            // Every byte to its stand-in character, then BPE over those.
            let mut symbols: Vec<String> = piece
                .bytes()
                .map(|b| self.to_char[b as usize].to_string())
                .collect();
            self.merge(&mut symbols);
            for symbol in symbols {
                match self.ids.get(&symbol) {
                    Some(id) => out.push(*id),
                    // A symbol with no id means the merges produced something
                    // the vocabulary does not contain, which cannot happen for
                    // a consistent pair of lists. Dropping it silently would
                    // shift every downstream position, so it is worth being
                    // loud about in a way a reader will see.
                    None => debug_assert!(false, "no id for token {symbol:?}"),
                }
            }
        }
        out
    }

    /// Apply merge rules until none applies.
    ///
    /// The lowest-ranked applicable merge, repeatedly. Not the leftmost: rank
    /// order is what makes the result independent of where in the word the
    /// merge happens to be, and it is what the model was trained against.
    fn merge(&self, symbols: &mut Vec<String>) {
        loop {
            let mut best: Option<(usize, u32)> = None;
            for i in 0..symbols.len().saturating_sub(1) {
                let pair = (symbols[i].clone(), symbols[i + 1].clone());
                if let Some(&rank) = self.merges.get(&pair) {
                    if best.is_none_or(|(_, current)| rank < current) {
                        best = Some((i, rank));
                    }
                }
            }
            let Some((at, _)) = best else { return };
            let joined = format!("{}{}", symbols[at], symbols[at + 1]);
            symbols[at] = joined;
            symbols.remove(at + 1);
        }
    }

    /// Turn token ids back into text.
    ///
    /// Lossy at the edges by construction: a single token can be half of a
    /// multi-byte character, so the bytes are gathered first and decoded once
    /// at the end rather than per token.
    pub fn decode(&self, ids: &[u32]) -> String {
        let mut bytes = Vec::new();
        for id in ids {
            let Some(token) = self.tokens.get(*id as usize) else {
                continue;
            };
            for ch in token.chars() {
                // A character with no reverse mapping belongs to a special
                // token such as `<|eot_id|>`, which is not byte-level text.
                // Skipped rather than rendered, because printing the marker is
                // not what the model said.
                if let Some(byte) = self.from_char.get(&ch) {
                    bytes.push(*byte);
                }
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

/// The byte-to-character substitution, and its inverse.
///
/// Printable ASCII and the two printable Latin-1 ranges stand for themselves;
/// everything else is displaced to U+0100 and upwards, in order. The exact
/// table matters — it is part of the vocabulary the model was trained with, not
/// a convention this file is free to choose.
fn byte_char_tables() -> ([char; 256], HashMap<char, u8>) {
    let mut to_char = ['\0'; 256];
    let mut taken = [false; 256];

    // The bytes that are already printable and unambiguous.
    for b in b'!'..=b'~' {
        to_char[b as usize] = b as char;
        taken[b as usize] = true;
    }
    for b in 0xA1u8..=0xAC {
        to_char[b as usize] = b as char;
        taken[b as usize] = true;
    }
    for b in 0xAEu8..=0xFF {
        to_char[b as usize] = b as char;
        taken[b as usize] = true;
    }

    // Everything else, displaced in order.
    let mut next = 0u32;
    for b in 0..256usize {
        if !taken[b] {
            to_char[b] = char::from_u32(256 + next).expect("inside the BMP");
            next += 1;
        }
    }

    let mut from_char = HashMap::with_capacity(256);
    for (b, ch) in to_char.iter().enumerate() {
        from_char.insert(*ch, b as u8);
    }
    (to_char, from_char)
}

/// Cut `text` into pieces that a merge may not cross.
///
/// The reference rules, in the reference order:
///
/// 1. an English contraction — `'s`, `'t`, `'re`, `'ve`, `'m`, `'ll`, `'d`
/// 2. an optional leading non-letter, non-digit, then a run of letters
/// 3. one to three digits
/// 4. an optional leading space, then a run of punctuation, then any newlines
/// 5. a run of newlines
/// 6. a run of spaces, keeping the last one for the word that follows
fn pre_tokenize(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let start = i;

        // 1. A contraction.
        if chars[i] == '\'' {
            if let Some(len) = contraction(&chars[i..]) {
                i += len;
                out.push(chars[start..i].iter().collect());
                continue;
            }
        }

        // 2. An optional single leading non-alphanumeric, then letters.
        let lead = usize::from(
            !chars[i].is_alphanumeric() && !chars[i].is_whitespace() && chars[i] != '\'',
        );
        if i + lead < chars.len() && chars[i + lead].is_alphabetic() {
            i += lead;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
            continue;
        }
        // The same, with a space in front — which is the ordinary case for
        // every word after the first.
        if chars[i] == ' ' && i + 1 < chars.len() && chars[i + 1].is_alphabetic() {
            i += 1;
            while i < chars.len() && chars[i].is_alphabetic() {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
            continue;
        }

        // 3. Digits, at most three at a time.
        if chars[i].is_ascii_digit() || (chars[i] == ' ' && next_is_digit(&chars, i + 1)) {
            if chars[i] == ' ' {
                i += 1;
            }
            let mut taken = 0;
            while i < chars.len() && chars[i].is_ascii_digit() && taken < 3 {
                i += 1;
                taken += 1;
            }
            out.push(chars[start..i].iter().collect());
            continue;
        }

        // 4. Punctuation, optionally with one space in front, then newlines.
        let space = usize::from(chars[i] == ' ');
        if i + space < chars.len() && is_punctuation(chars[i + space]) {
            i += space;
            while i < chars.len() && is_punctuation(chars[i]) {
                i += 1;
            }
            while i < chars.len() && (chars[i] == '\r' || chars[i] == '\n') {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
            continue;
        }

        // 5. Newlines.
        if chars[i] == '\r' || chars[i] == '\n' {
            while i < chars.len() && (chars[i] == '\r' || chars[i] == '\n') {
                i += 1;
            }
            out.push(chars[start..i].iter().collect());
            continue;
        }

        // 6. Whitespace. A run of spaces before a word keeps its last space for
        // that word, which is what makes " the" one token rather than two.
        if chars[i].is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            let keeps_last = i < chars.len() && !chars[i - 1].is_whitespace();
            let end = if keeps_last { i - 1 } else { i };
            if end > start {
                out.push(chars[start..end].iter().collect());
            }
            i = end.max(start + 1).min(i);
            continue;
        }

        // Anything else, one character at a time, so this always terminates.
        i += 1;
        out.push(chars[start..i].iter().collect());
    }

    out
}

fn next_is_digit(chars: &[char], at: usize) -> bool {
    chars.get(at).is_some_and(char::is_ascii_digit)
}

fn is_punctuation(ch: char) -> bool {
    !ch.is_alphanumeric() && !ch.is_whitespace()
}

/// How many characters of an English contraction start `chars`, if any.
fn contraction(chars: &[char]) -> Option<usize> {
    const FORMS: [&str; 7] = ["'s", "'t", "'re", "'ve", "'m", "'ll", "'d"];
    let text: String = chars.iter().take(3).collect::<String>().to_lowercase();
    FORMS
        .iter()
        .filter(|form| text.starts_with(*form))
        .map(|form| form.chars().count())
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_byte_table_is_a_bijection() {
        let (to_char, from_char) = byte_char_tables();
        assert_eq!(from_char.len(), 256, "every byte needs its own character");
        for b in 0..256usize {
            assert_eq!(from_char[&to_char[b]], b as u8);
        }
    }

    /// The one that matters for a prompt: a space belongs to the word after it.
    #[test]
    fn a_space_joins_the_word_that_follows() {
        assert_eq!(pre_tokenize("the cat"), vec!["the", " cat"]);
    }

    #[test]
    fn punctuation_is_its_own_piece() {
        assert_eq!(pre_tokenize("hi, there!"), vec!["hi", ",", " there", "!"]);
    }

    #[test]
    fn digits_come_in_threes_at_most() {
        assert_eq!(pre_tokenize("12345"), vec!["123", "45"]);
    }

    #[test]
    fn a_contraction_stays_whole() {
        assert_eq!(pre_tokenize("it's"), vec!["it", "'s"]);
    }

    /// Every path has to consume something, or encoding hangs on the first
    /// character it does not recognise.
    #[test]
    fn pre_tokenizing_always_terminates_and_loses_nothing() {
        for text in [
            "",
            " ",
            "   ",
            "\n\n",
            "a",
            "!",
            "  \n  x",
            "The capital of France is",
            "naïve café — 42 things",
        ] {
            let pieces = pre_tokenize(text);
            assert_eq!(
                pieces.concat(),
                text,
                "the pieces of {text:?} should rejoin into it"
            );
        }
    }
}
