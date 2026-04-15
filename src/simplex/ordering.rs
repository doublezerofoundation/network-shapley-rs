use super::sparse::Perm;

/// Pre-order columns by ascending number of non-zeros (sparsest first).
///
/// This heuristic reduces fill-in during LU factorisation — processing
/// sparser columns first tends to produce sparser L and U factors.
/// Returns a [`Perm`] mapping between original and reordered column indices.
pub fn order_simple<'a>(size: usize, get_col: impl Fn(usize) -> &'a [usize]) -> Perm {
    let mut cols_queue = ColsQueue::new(size);
    for c in 0..size {
        cols_queue.add(c, get_col(c).len() - 1);
    }

    let mut new2orig = Vec::with_capacity(size);

    //TODO should this be refactored?
    while new2orig.len() < size {
        let min = cols_queue.pop_min();
        //guaranteed to exist
        new2orig.push(min.unwrap());
    }

    let mut orig2new = vec![0; size];
    for (new, &orig) in new2orig.iter().enumerate() {
        orig2new[orig] = new;
    }

    Perm { orig2new, new2orig }
}

/// A priority queue of columns keyed by their "score" (non-zero count).
///
/// Implemented as an array of circular doubly-linked lists — one list per
/// score value. `pop_min()` returns the column with the smallest score,
/// which is the sparsest column. This is efficient because scores are
/// small non-negative integers bounded by the matrix dimension.
#[derive(Debug)]
struct ColsQueue {
    /// Head pointer for each score bucket (None = empty bucket).
    score2head: Vec<Option<usize>>,
    /// Prev/next pointers forming circular doubly-linked lists within each bucket.
    prev: Vec<usize>,
    next: Vec<usize>,
    /// Smallest score that has any columns in it (avoids scanning empty buckets).
    min_score: usize,
    len: usize,
}

impl ColsQueue {
    fn new(num_cols: usize) -> ColsQueue {
        ColsQueue {
            score2head: vec![None; num_cols],
            prev: vec![0; num_cols],
            next: vec![0; num_cols],
            min_score: num_cols,
            len: 0,
        }
    }

    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.len
    }

    fn pop_min(&mut self) -> Option<usize> {
        let col = loop {
            if self.min_score >= self.score2head.len() {
                return None;
            }
            if let Some(col) = self.score2head[self.min_score] {
                break col;
            }
            self.min_score += 1;
        };

        self.remove(col, self.min_score);
        Some(col)
    }

    fn add(&mut self, col: usize, score: usize) {
        self.min_score = std::cmp::min(self.min_score, score);
        self.len += 1;

        if let Some(head) = self.score2head[score] {
            self.prev[col] = self.prev[head];
            self.next[col] = head;
            self.next[self.prev[head]] = col;
            self.prev[head] = col;
        } else {
            self.prev[col] = col;
            self.next[col] = col;
            self.score2head[score] = Some(col);
        }
    }

    fn remove(&mut self, col: usize, score: usize) {
        self.len -= 1;
        if self.next[col] == col {
            self.score2head[score] = None;
        } else {
            self.next[self.prev[col]] = self.next[col];
            self.prev[self.next[col]] = self.prev[col];

            //will panic if score is not valid
            if self.score2head[score].unwrap() == col {
                self.score2head[score] = Some(self.next[col]);
            }
        }
    }
}
