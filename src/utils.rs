/// Check if a string contains any digit
pub(crate) fn has_digit(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_digit())
}

/// Generate a bitmap where column j is the binary representation of j
pub(crate) fn generate_bitmap(n_bits: usize) -> Vec<Vec<u8>> {
    let n_cols = 1 << n_bits;
    let mut bitmap = vec![vec![0u8; n_cols]; n_bits];

    for col in 0..n_cols {
        for (row, row_bitmap) in bitmap.iter_mut().enumerate().take(n_bits) {
            row_bitmap[col] = ((col >> row) & 1) as u8;
        }
    }

    bitmap
}

/// Calculate factorial (cached for small values)
pub(crate) const FACTORIAL_LIMIT: usize = 21;
pub(crate) const FACTORIALS: [u64; FACTORIAL_LIMIT] = {
    let mut facts = [1u64; FACTORIAL_LIMIT];
    let mut i = 1;
    while i < FACTORIAL_LIMIT {
        facts[i] = facts[i - 1] * (i as u64);
        i += 1;
    }
    facts
};

pub(crate) fn factorial(n: usize) -> f64 {
    if n < FACTORIAL_LIMIT {
        FACTORIALS[n] as f64
    } else {
        // Use Stirling's approximation for large n
        let n_f64 = n as f64;
        (2.0 * std::f64::consts::PI * n_f64).sqrt() * (n_f64 / std::f64::consts::E).powf(n_f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_digit() {
        assert!(has_digit("SIN1"));
        assert!(has_digit("FRA01"));
        assert!(!has_digit("SIN"));
        assert!(!has_digit("FRA"));
    }

    #[test]
    fn test_factorial() {
        assert_eq!(factorial(0), 1.0);
        assert_eq!(factorial(5), 120.0);
        assert_eq!(factorial(10), 3628800.0);
    }
}
