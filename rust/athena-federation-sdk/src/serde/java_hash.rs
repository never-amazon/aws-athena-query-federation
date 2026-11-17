use std::cmp::Ordering;

use crate::data::java_format::java_string_hash;

const DEFAULT_INITIAL_CAPACITY: usize = 16;
const MAXIMUM_CAPACITY: usize = 1 << 30;
const TREEIFY_THRESHOLD: usize = 8;
const UNTREEIFY_THRESHOLD: usize = 6;
const MIN_TREEIFY_CAPACITY: usize = 64;

fn spread_hash(hash: i32) -> i32 {
    hash ^ hash.cast_unsigned().wrapping_shr(16).cast_signed()
}

fn java_string_compare(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

fn table_size_for(cap: usize) -> usize {
    if cap <= 1 {
        return 1;
    }
    cap.next_power_of_two().min(MAXIMUM_CAPACITY)
}

#[derive(Clone, Copy, Default)]
struct Node {
    hash: i32,
    next: Option<usize>,
    prev: Option<usize>,
    parent: Option<usize>,
    left: Option<usize>,
    right: Option<usize>,
    red: bool,
}

#[derive(Clone, Copy)]
struct Chain {
    head: usize,
    tail: usize,
    len: usize,
}

#[derive(Clone, Copy)]
struct Bin {
    head: usize,
    tree: bool,
}

struct JavaHashMap<C> {
    nodes: Vec<Node>,
    table: Vec<Option<Bin>>,
    threshold: usize,
    size: usize,
    compare: C,
}

impl<C> JavaHashMap<C>
where
    C: Fn(usize, usize) -> Option<Ordering>,
{
    fn new(initial_threshold: usize, compare: C) -> Self {
        Self {
            nodes: Vec::new(),
            table: Vec::new(),
            threshold: initial_threshold,
            size: 0,
            compare,
        }
    }

    fn index(&self, hash: i32) -> usize {
        hash.cast_unsigned() as usize & (self.table.len() - 1)
    }

    fn insert(&mut self, hash: i32) {
        let id = self.nodes.len();
        self.nodes.push(Node {
            hash,
            ..Node::default()
        });
        if self.table.is_empty() {
            self.resize();
        }
        let i = self.index(hash);
        match self.table[i] {
            None => {
                self.table[i] = Some(Bin {
                    head: id,
                    tree: false,
                });
            }
            Some(Bin { head, tree: true }) => self.put_tree_val(head, id),
            Some(Bin { head, tree: false }) => {
                let mut p = head;
                let mut bin_count = 0;
                while let Some(e) = self.nodes[p].next {
                    p = e;
                    bin_count += 1;
                }
                self.nodes[p].next = Some(id);
                if bin_count >= TREEIFY_THRESHOLD - 1 {
                    self.treeify_bin(hash);
                }
            }
        }
        self.size += 1;
        if self.size > self.threshold {
            self.resize();
        }
    }

    fn resize(&mut self) {
        let old_cap = self.table.len();
        let old_thr = self.threshold;
        let mut new_thr = 0;
        let new_cap = if old_cap > 0 {
            if old_cap >= MAXIMUM_CAPACITY {
                self.threshold = usize::MAX;
                return;
            }
            let doubled = old_cap << 1;
            if doubled < MAXIMUM_CAPACITY && old_cap >= DEFAULT_INITIAL_CAPACITY {
                new_thr = old_thr << 1;
            }
            doubled
        } else if old_thr > 0 {
            old_thr
        } else {
            new_thr = DEFAULT_INITIAL_CAPACITY * 3 / 4;
            DEFAULT_INITIAL_CAPACITY
        };
        if new_thr == 0 {
            new_thr = if new_cap < MAXIMUM_CAPACITY {
                new_cap * 3 / 4
            } else {
                usize::MAX
            };
        }
        self.threshold = new_thr;
        let old_table = std::mem::replace(&mut self.table, vec![None; new_cap]);
        for (j, bin) in old_table.into_iter().enumerate() {
            let Some(bin) = bin else { continue };
            if self.nodes[bin.head].next.is_none() {
                let i = self.index(self.nodes[bin.head].hash);
                self.table[i] = Some(bin);
            } else if bin.tree {
                self.split(bin.head, j, old_cap);
            } else {
                let [lo, hi] = self.relink(bin.head, old_cap);
                for (slot, chain) in [(j, lo), (j + old_cap, hi)] {
                    if let Some(chain) = chain {
                        self.table[slot] = Some(Bin {
                            head: chain.head,
                            tree: false,
                        });
                    }
                }
            }
        }
    }

    fn relink(&mut self, head: usize, bit: usize) -> [Option<Chain>; 2] {
        let mut halves: [Option<Chain>; 2] = [None, None];
        let mut cursor = Some(head);
        while let Some(e) = cursor {
            cursor = self.nodes[e].next;
            self.nodes[e].next = None;
            let half = usize::from(self.nodes[e].hash.cast_unsigned() as usize & bit != 0);
            match &mut halves[half] {
                None => {
                    self.nodes[e].prev = None;
                    halves[half] = Some(Chain {
                        head: e,
                        tail: e,
                        len: 1,
                    });
                }
                Some(chain) => {
                    self.nodes[e].prev = Some(chain.tail);
                    self.nodes[chain.tail].next = Some(e);
                    chain.tail = e;
                    chain.len += 1;
                }
            }
        }
        halves
    }

    fn split(&mut self, head: usize, index: usize, bit: usize) {
        let [lo, hi] = self.relink(head, bit);
        for (slot, this, other) in [(index, lo, hi), (index + bit, hi, lo)] {
            let Some(Chain { head, len, .. }) = this else {
                continue;
            };
            if len <= UNTREEIFY_THRESHOLD {
                self.table[slot] = Some(Bin { head, tree: false });
            } else {
                self.table[slot] = Some(Bin { head, tree: true });
                if other.is_some() {
                    self.treeify(head);
                }
            }
        }
    }

    fn treeify_bin(&mut self, hash: i32) {
        if self.table.len() < MIN_TREEIFY_CAPACITY {
            self.resize();
            return;
        }
        let index = self.index(hash);
        let Some(bin) = self.table[index] else { return };
        let mut prev = None;
        let mut cursor = Some(bin.head);
        while let Some(e) = cursor {
            self.nodes[e].prev = prev;
            prev = Some(e);
            cursor = self.nodes[e].next;
        }
        self.table[index] = Some(Bin {
            head: bin.head,
            tree: true,
        });
        self.treeify(bin.head);
    }

    fn direction(&self, x: usize, p: usize) -> Ordering {
        let h = self.nodes[x].hash;
        let ph = self.nodes[p].hash;
        match ph.cmp(&h) {
            Ordering::Greater => Ordering::Less,
            Ordering::Less => Ordering::Greater,
            Ordering::Equal => match (self.compare)(x, p) {
                Some(Ordering::Equal) | None => Ordering::Less,
                Some(dir) => dir,
            },
        }
    }

    fn attach(&mut self, x: usize, root: usize) -> usize {
        let mut p = root;
        loop {
            let dir = self.direction(x, p);
            let child = if dir == Ordering::Greater {
                self.nodes[p].right
            } else {
                self.nodes[p].left
            };
            if let Some(c) = child {
                p = c;
                continue;
            }
            if dir == Ordering::Greater {
                self.nodes[p].right = Some(x);
            } else {
                self.nodes[p].left = Some(x);
            }
            self.nodes[x].parent = Some(p);
            return p;
        }
    }

    fn treeify(&mut self, head: usize) {
        let mut root: Option<usize> = None;
        let mut cursor = Some(head);
        while let Some(x) = cursor {
            cursor = self.nodes[x].next;
            self.nodes[x].left = None;
            self.nodes[x].right = None;
            root = Some(match root {
                None => {
                    self.nodes[x].parent = None;
                    self.nodes[x].red = false;
                    x
                }
                Some(r) => {
                    self.attach(x, r);
                    self.balance_insertion(r, x)
                }
            });
        }
        if let Some(r) = root {
            self.move_root_to_front(r);
        }
    }

    fn put_tree_val(&mut self, first: usize, x: usize) {
        let mut root = first;
        while let Some(p) = self.nodes[root].parent {
            root = p;
        }
        let xp = self.attach(x, root);
        let xpn = self.nodes[xp].next;
        self.nodes[x].next = xpn;
        self.nodes[xp].next = Some(x);
        self.nodes[x].prev = Some(xp);
        if let Some(n) = xpn {
            self.nodes[n].prev = Some(x);
        }
        let root = self.balance_insertion(root, x);
        self.move_root_to_front(root);
    }

    fn move_root_to_front(&mut self, root: usize) {
        let index = self.index(self.nodes[root].hash);
        let first = self.table[index].map(|b| b.head);
        if first == Some(root) {
            return;
        }
        self.table[index] = Some(Bin {
            head: root,
            tree: true,
        });
        let rp = self.nodes[root].prev;
        let rn = self.nodes[root].next;
        if let Some(n) = rn {
            self.nodes[n].prev = rp;
        }
        if let Some(p) = rp {
            self.nodes[p].next = rn;
        }
        if let Some(f) = first {
            self.nodes[f].prev = Some(root);
        }
        self.nodes[root].next = first;
        self.nodes[root].prev = None;
    }

    fn rotate_left(&mut self, mut root: usize, p: usize) -> usize {
        let Some(r) = self.nodes[p].right else {
            return root;
        };
        let rl = self.nodes[r].left;
        self.nodes[p].right = rl;
        if let Some(rl) = rl {
            self.nodes[rl].parent = Some(p);
        }
        let pp = self.nodes[p].parent;
        self.nodes[r].parent = pp;
        match pp {
            None => {
                root = r;
                self.nodes[r].red = false;
            }
            Some(pp) if self.nodes[pp].left == Some(p) => self.nodes[pp].left = Some(r),
            Some(pp) => self.nodes[pp].right = Some(r),
        }
        self.nodes[r].left = Some(p);
        self.nodes[p].parent = Some(r);
        root
    }

    fn rotate_right(&mut self, mut root: usize, p: usize) -> usize {
        let Some(l) = self.nodes[p].left else {
            return root;
        };
        let lr = self.nodes[l].right;
        self.nodes[p].left = lr;
        if let Some(lr) = lr {
            self.nodes[lr].parent = Some(p);
        }
        let pp = self.nodes[p].parent;
        self.nodes[l].parent = pp;
        match pp {
            None => {
                root = l;
                self.nodes[l].red = false;
            }
            Some(pp) if self.nodes[pp].right == Some(p) => self.nodes[pp].right = Some(l),
            Some(pp) => self.nodes[pp].left = Some(l),
        }
        self.nodes[l].right = Some(p);
        self.nodes[p].parent = Some(l);
        root
    }

    fn is_red(&self, node: Option<usize>) -> bool {
        node.is_some_and(|n| self.nodes[n].red)
    }

    fn balance_insertion(&mut self, mut root: usize, mut x: usize) -> usize {
        self.nodes[x].red = true;
        loop {
            let Some(mut xp) = self.nodes[x].parent else {
                self.nodes[x].red = false;
                return x;
            };
            if !self.nodes[xp].red {
                return root;
            }
            let Some(mut xpp) = self.nodes[xp].parent else {
                return root;
            };
            let parent_is_left = self.nodes[xpp].left == Some(xp);
            let uncle = if parent_is_left {
                self.nodes[xpp].right
            } else {
                self.nodes[xpp].left
            };
            if self.is_red(uncle) {
                if let Some(u) = uncle {
                    self.nodes[u].red = false;
                }
                self.nodes[xp].red = false;
                self.nodes[xpp].red = true;
                x = xpp;
                continue;
            }
            let inner = if parent_is_left {
                self.nodes[xp].right == Some(x)
            } else {
                self.nodes[xp].left == Some(x)
            };
            let mut grandparent = Some(xpp);
            let mut parent = Some(xp);
            if inner {
                x = xp;
                root = if parent_is_left {
                    self.rotate_left(root, x)
                } else {
                    self.rotate_right(root, x)
                };
                parent = self.nodes[x].parent;
                grandparent = parent.and_then(|p| self.nodes[p].parent);
            }
            if let Some(p) = parent {
                xp = p;
                self.nodes[xp].red = false;
                if let Some(g) = grandparent {
                    xpp = g;
                    self.nodes[xpp].red = true;
                    root = if parent_is_left {
                        self.rotate_right(root, xpp)
                    } else {
                        self.rotate_left(root, xpp)
                    };
                }
            }
        }
    }

    fn iteration_order(&self) -> Vec<usize> {
        let mut order = Vec::with_capacity(self.nodes.len());
        for bin in self.table.iter().flatten() {
            let mut cursor = Some(bin.head);
            while let Some(e) = cursor {
                order.push(e);
                cursor = self.nodes[e].next;
            }
        }
        order
    }
}

fn java_index_order<C>(hashes: &[i32], initial_threshold: usize, compare: C) -> Vec<usize>
where
    C: Fn(usize, usize) -> Option<Ordering>,
{
    let mut map = JavaHashMap::new(initial_threshold, compare);
    for hash in hashes {
        map.insert(spread_hash(*hash));
    }
    map.iteration_order()
}

fn reorder<T>(items: Vec<T>, order: Vec<usize>) -> Vec<T> {
    let mut slots: Vec<Option<T>> = items.into_iter().map(Some).collect();
    order.into_iter().filter_map(|i| slots[i].take()).collect()
}

fn java_string_index_order(keys: &[&str], initial_threshold: usize) -> Vec<usize> {
    let hashes: Vec<i32> = keys.iter().map(|k| java_string_hash(k)).collect();
    java_index_order(&hashes, initial_threshold, |a, b| {
        Some(java_string_compare(keys[a], keys[b]))
    })
}

fn dedup<'a, I>(keys: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut distinct: Vec<&'a str> = Vec::new();
    for k in keys {
        if !distinct.contains(&k) {
            distinct.push(k);
        }
    }
    distinct
}

pub fn java_hash_order_with_table<'a, I>(
    keys_in_insertion_order: I,
    initial_table: usize,
) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let distinct = dedup(keys_in_insertion_order);
    let order = java_string_index_order(&distinct, initial_table);
    reorder(distinct, order)
}

pub fn java_hash_set_order_by_hash<T, F>(distinct_in_insertion_order: Vec<T>, hash: F) -> Vec<T>
where
    F: Fn(&T) -> i32,
{
    let hashes: Vec<i32> = distinct_in_insertion_order.iter().map(&hash).collect();
    let order = java_index_order(&hashes, 0, |_, _| None);
    reorder(distinct_in_insertion_order, order)
}

pub fn java_hash_set_order_by_hash_with_compare<T, F, C>(
    distinct_in_insertion_order: Vec<T>,
    hash: F,
    compare: C,
) -> Vec<T>
where
    F: Fn(&T) -> i32,
    C: Fn(&T, &T) -> Ordering,
{
    let items = &distinct_in_insertion_order;
    let hashes: Vec<i32> = items.iter().map(&hash).collect();
    let order = java_index_order(&hashes, 0, |a, b| Some(compare(&items[a], &items[b])));
    reorder(distinct_in_insertion_order, order)
}

pub fn java_hash_order_default<'a, I>(keys_in_insertion_order: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    java_hash_order_with_table(keys_in_insertion_order, 0)
}

pub fn java_hash_order_with_capacity<'a, I>(
    keys_in_insertion_order: I,
    initial_capacity: usize,
) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    java_hash_order_with_table(keys_in_insertion_order, table_size_for(initial_capacity))
}

pub fn java_hash_set_copy_order<'a, I>(keys_in_insertion_order: I) -> Vec<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let keys: Vec<&'a str> = keys_in_insertion_order.into_iter().collect();
    let capacity = std::cmp::max(keys.len() * 4 / 3 + 1, 16);
    java_hash_order_with_capacity(keys, capacity)
}

pub fn java_hash_map_order<'a, I>(keys: I) -> Vec<&'a String>
where
    I: IntoIterator<Item = &'a String>,
{
    let mut sorted: Vec<&'a String> = keys.into_iter().collect();
    sorted.sort_by(|a, b| java_string_compare(a, b));
    sorted.dedup();
    let keys: Vec<&str> = sorted.iter().map(|k| k.as_str()).collect();
    let order = java_string_index_order(&keys, 0);
    reorder(sorted, order)
}

#[cfg(test)]
mod tests {
    use super::java_hash_set_copy_order;

    #[test]
    fn hash_set_copy_order_matches_java_for_twelve_keys() {
        let keys: Vec<String> = (0..12).map(|i| format!("col{i}")).collect();
        let order = java_hash_set_copy_order(keys.iter().map(String::as_str));
        assert_eq!(
            "col11,col8,col9,col6,col10,col7,col4,col5,col2,col3,col0,col1",
            order.join(",")
        );
    }
}
