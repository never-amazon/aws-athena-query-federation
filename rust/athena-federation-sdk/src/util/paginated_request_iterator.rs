use crate::exceptions::{FederationError, FederationSourceErrorCode, Result};

pub struct PaginatedRequestIterator<T, F, G>
where
    F: FnMut(Option<&str>) -> T,
    G: FnMut(&T) -> Option<String>,
{
    fetch_page: F,
    get_token: G,
    not_started: bool,
    next_page_token: Option<String>,
}

impl<T, F, G> PaginatedRequestIterator<T, F, G>
where
    F: FnMut(Option<&str>) -> T,
    G: FnMut(&T) -> Option<String>,
{
    pub fn new(fetch_page: F, get_token: G) -> Self {
        PaginatedRequestIterator {
            fetch_page,
            get_token,
            not_started: true,
            next_page_token: None,
        }
    }

    pub fn stream(fetch_page: F, get_token: G) -> impl Iterator<Item = T> {
        let mut it = PaginatedRequestIterator::new(fetch_page, get_token);
        std::iter::from_fn(move || if it.has_next() { it.next_page().ok() } else { None })
    }

    pub fn has_next(&self) -> bool {
        self.not_started || self.next_page_token.is_some()
    }

    pub fn next_page(&mut self) -> Result<T> {
        if !self.has_next() {
            return Err(FederationError::connector(
                FederationSourceErrorCode::EntityNotFoundException,
                "No more pages left",
            ));
        }
        let current = (self.fetch_page)(self.next_page_token.as_deref());
        self.not_started = false;
        self.next_page_token = (self.get_token)(&current);
        Ok(current)
    }
}
