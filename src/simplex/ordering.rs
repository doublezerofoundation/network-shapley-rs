use super::sparse::Perm;

/// Simplest preordering: order columns based on their size
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

#[derive(Debug)]
struct ColsQueue {
    score2head: Vec<Option<usize>>,
    prev: Vec<usize>,
    next: Vec<usize>,
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
