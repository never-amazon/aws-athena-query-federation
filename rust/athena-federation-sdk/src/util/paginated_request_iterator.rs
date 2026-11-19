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
}

impl<T, F, G> Iterator for PaginatedRequestIterator<T, F, G>
where
    F: FnMut(Option<&str>) -> T,
    G: FnMut(&T) -> Option<String>,
{
    type Item = T;

    fn next(&mut self) -> Option<T> {
        if !self.not_started && self.next_page_token.is_none() {
            return None;
        }
        let current = (self.fetch_page)(self.next_page_token.as_deref());
        self.not_started = false;
        self.next_page_token = (self.get_token)(&current);
        Some(current)
    }
}
