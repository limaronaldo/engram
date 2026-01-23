//! Fuzzy search with typo tolerance (RML-877)
//!
//! Uses Levenshtein distance for typo correction and suggestion.

use std::collections::{HashMap, HashSet};

/// Maximum edit distance for fuzzy matching
const MAX_EDIT_DISTANCE: usize = 2;

/// Minimum word length to apply fuzzy matching
const MIN_WORD_LENGTH: usize = 4;

/// Fuzzy search engine
pub struct FuzzyEngine {
    /// Vocabulary built from indexed content
    vocabulary: HashSet<String>,
    /// Word frequency for ranking suggestions
    word_freq: HashMap<String, usize>,
}

impl FuzzyEngine {
    /// Create a new fuzzy engine
    pub fn new() -> Self {
        Self {
            vocabulary: HashSet::new(),
            word_freq: HashMap::new(),
        }
    }

    /// Add text to the vocabulary
    pub fn add_to_vocabulary(&mut self, text: &str) {
        for word in tokenize(text) {
            if word.len() >= MIN_WORD_LENGTH {
                self.vocabulary.insert(word.clone());
                *self.word_freq.entry(word).or_insert(0) += 1;
            }
        }
    }

    /// Find corrections for a query
    pub fn correct_query(&self, query: &str) -> CorrectionResult {
        let mut corrections = Vec::new();
        let mut corrected_query = String::new();
        let mut had_corrections = false;

        for word in query.split_whitespace() {
            let word_lower = word.to_lowercase();

            // Skip short words or words already in vocabulary
            if word_lower.len() < MIN_WORD_LENGTH || self.vocabulary.contains(&word_lower) {
                if !corrected_query.is_empty() {
                    corrected_query.push(' ');
                }
                corrected_query.push_str(word);
                continue;
            }

            // Find best correction
            if let Some(correction) = self.find_best_correction(&word_lower) {
                corrections.push(Correction {
                    original: word.to_string(),
                    corrected: correction.clone(),
                    distance: levenshtein(&word_lower, &correction),
                });
                had_corrections = true;

                if !corrected_query.is_empty() {
                    corrected_query.push(' ');
                }
                corrected_query.push_str(&correction);
            } else {
                if !corrected_query.is_empty() {
                    corrected_query.push(' ');
                }
                corrected_query.push_str(word);
            }
        }

        CorrectionResult {
            original_query: query.to_string(),
            corrected_query: if had_corrections {
                Some(corrected_query)
            } else {
                None
            },
            corrections,
            suggestions: self.get_suggestions(query, 5),
        }
    }

    /// Find the best correction for a word
    fn find_best_correction(&self, word: &str) -> Option<String> {
        let mut best: Option<(String, usize, usize)> = None; // (word, distance, frequency)

        for vocab_word in &self.vocabulary {
            let distance = levenshtein(word, vocab_word);

            if distance <= MAX_EDIT_DISTANCE {
                let freq = *self.word_freq.get(vocab_word).unwrap_or(&0);

                match &best {
                    None => {
                        best = Some((vocab_word.clone(), distance, freq));
                    }
                    Some((_, best_dist, best_freq)) => {
                        // Prefer smaller distance, then higher frequency
                        if distance < *best_dist || (distance == *best_dist && freq > *best_freq) {
                            best = Some((vocab_word.clone(), distance, freq));
                        }
                    }
                }
            }
        }

        best.map(|(word, _, _)| word)
    }

    /// Get search suggestions based on prefix matching and similarity
    fn get_suggestions(&self, query: &str, limit: usize) -> Vec<String> {
        let query_lower = query.to_lowercase();
        let mut suggestions: Vec<(String, usize)> = Vec::new();

        for word in &self.vocabulary {
            // Prefix match
            if word.starts_with(&query_lower) {
                let freq = *self.word_freq.get(word).unwrap_or(&0);
                suggestions.push((word.clone(), freq));
            }
            // Similar words
            else if query_lower.len() >= MIN_WORD_LENGTH {
                let distance = levenshtein(&query_lower, word);
                if distance <= MAX_EDIT_DISTANCE {
                    let freq = *self.word_freq.get(word).unwrap_or(&0);
                    suggestions.push((word.clone(), freq));
                }
            }
        }

        // Sort by frequency (descending)
        suggestions.sort_by(|a, b| b.1.cmp(&a.1));

        suggestions
            .into_iter()
            .take(limit)
            .map(|(word, _)| word)
            .collect()
    }

    /// Get vocabulary size
    pub fn vocabulary_size(&self) -> usize {
        self.vocabulary.len()
    }
}

impl Default for FuzzyEngine {
    fn default() -> Self {
        Self::new()
    }
}

use serde::{Deserialize, Serialize};

/// Result of query correction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionResult {
    pub original_query: String,
    pub corrected_query: Option<String>,
    pub corrections: Vec<Correction>,
    pub suggestions: Vec<String>,
}

/// A single word correction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Correction {
    pub original: String,
    pub corrected: String,
    pub distance: usize,
}

/// Calculate Levenshtein distance between two strings
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    // Use two rows instead of full matrix for memory efficiency
    let mut prev_row: Vec<usize> = (0..=b_len).collect();
    let mut curr_row: Vec<usize> = vec![0; b_len + 1];

    for i in 1..=a_len {
        curr_row[0] = i;

        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };

            curr_row[j] = (prev_row[j] + 1) // deletion
                .min(curr_row[j - 1] + 1) // insertion
                .min(prev_row[j - 1] + cost); // substitution
        }

        std::mem::swap(&mut prev_row, &mut curr_row);
    }

    prev_row[b_len]
}

/// Damerau-Levenshtein distance (includes transpositions)
pub fn damerau_levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let a_len = a_chars.len();
    let b_len = b_chars.len();

    if a_len == 0 {
        return b_len;
    }
    if b_len == 0 {
        return a_len;
    }

    let mut matrix: Vec<Vec<usize>> = vec![vec![0; b_len + 1]; a_len + 1];

    for i in 0..=a_len {
        matrix[i][0] = i;
    }
    for j in 0..=b_len {
        matrix[0][j] = j;
    }

    for i in 1..=a_len {
        for j in 1..=b_len {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };

            matrix[i][j] = (matrix[i - 1][j] + 1) // deletion
                .min(matrix[i][j - 1] + 1) // insertion
                .min(matrix[i - 1][j - 1] + cost); // substitution

            // Transposition
            if i > 1
                && j > 1
                && a_chars[i - 1] == b_chars[j - 2]
                && a_chars[i - 2] == b_chars[j - 1]
            {
                matrix[i][j] = matrix[i][j].min(matrix[i - 2][j - 2] + cost);
            }
        }
    }

    matrix[a_len][b_len]
}

/// Tokenize text into lowercase words
fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("hello", "hello"), 0);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", ""), 3);
    }

    #[test]
    fn test_damerau_levenshtein() {
        assert_eq!(damerau_levenshtein("ab", "ba"), 1); // transposition
        assert_eq!(damerau_levenshtein("hello", "hlelo"), 1); // transposition
    }

    #[test]
    fn test_fuzzy_engine() {
        let mut engine = FuzzyEngine::new();
        engine.add_to_vocabulary("authentication");
        engine.add_to_vocabulary("authorization");
        engine.add_to_vocabulary("automatic");

        let result = engine.correct_query("authentcation"); // typo
        assert!(result.corrected_query.is_some());
        assert_eq!(result.corrected_query.unwrap(), "authentication");
    }

    #[test]
    fn test_suggestions() {
        let mut engine = FuzzyEngine::new();
        engine.add_to_vocabulary("authentication");
        engine.add_to_vocabulary("authorization");
        engine.add_to_vocabulary("automatic");

        let suggestions = engine.get_suggestions("auth", 5);
        assert!(!suggestions.is_empty());
        assert!(suggestions.iter().any(|s| s.starts_with("auth")));
    }
}
