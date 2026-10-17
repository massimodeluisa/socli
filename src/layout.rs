//! Thread a flat page of posts into the rails the reader draws.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::model::Post;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Post(String),
    Fold {
        parent: String,
        hidden: usize,
    },
    Gap,
    /// The account header above one account's posts.
    Profile,
}

#[derive(Clone, Debug)]
pub struct LaidRow {
    pub target: Target,
    pub depth: usize,
    /// Draw the vertical rail under this row's own column (children follow).
    pub own_rail: bool,
    pub number: Option<usize>,
    /// Loaded replies the split's overview leaves out under this root; 0 everywhere else.
    pub folded: usize,
}

/// The deepest level a reply nests at; anything below hangs from its ancestor at this depth.
const MAX_DEPTH: usize = 48;

struct Node {
    id: String,
    children: Vec<Node>,
    newest: Option<DateTime<Utc>>,
}

/// `newest` puts each post's newest reply first instead of its oldest.
pub fn lay(posts: &[Post], expanded: &HashSet<String>, newest: bool) -> Vec<LaidRow> {
    let nodes = forest(posts, newest);
    let mut rows = Vec::new();
    let mut number = 1usize;
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 {
            rows.push(gap());
        }
        number = walk(node, 0, expanded, &mut number, &mut rows);
    }
    rows
}

/// The rows of [`lay`], or of [`overview`] while the split's feed pane is drawn.
pub fn lay_for(
    posts: &[Post],
    expanded: &HashSet<String>,
    newest: bool,
    overview_on: bool,
) -> Vec<LaidRow> {
    if overview_on {
        overview(posts, expanded, newest)
    } else {
        lay(posts, expanded, newest)
    }
}

/**
The split's feed pane: each conversation's root alone, counting its loaded replies in `folded`. A
root in `expanded` is laid out as [`lay`] lays it out.
*/
pub fn overview(posts: &[Post], expanded: &HashSet<String>, newest: bool) -> Vec<LaidRow> {
    let nodes = forest(posts, newest);
    let mut rows = Vec::new();
    let mut number = 1usize;
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 {
            rows.push(gap());
        }
        if expanded.contains(&node.id) || node.children.is_empty() {
            number = walk(node, 0, expanded, &mut number, &mut rows);
            continue;
        }
        rows.push(LaidRow {
            target: Target::Post(node.id.clone()),
            depth: 0,
            own_rail: false,
            number: Some(number),
            folded: descendants(node),
        });
        number += 1;
    }
    rows
}

fn descendants(node: &Node) -> usize {
    node.children
        .iter()
        .map(|child| 1 + descendants(child))
        .sum()
}

/// `rows` under a profile header, which becomes the first selectable row.
pub fn profiled(rows: Vec<LaidRow>) -> Vec<LaidRow> {
    let header = LaidRow {
        target: Target::Profile,
        depth: 0,
        own_rail: false,
        number: None,
        folded: 0,
    };
    let mut out = vec![header];
    if !rows.is_empty() {
        out.push(gap());
    }
    out.extend(rows);
    out
}

fn gap() -> LaidRow {
    LaidRow {
        target: Target::Gap,
        depth: 0,
        own_rail: false,
        number: None,
        folded: 0,
    }
}

pub fn is_gap(row: &LaidRow) -> bool {
    matches!(row.target, Target::Gap)
}

fn walk(
    node: &Node,
    depth: usize,
    expanded: &HashSet<String>,
    number: &mut usize,
    rows: &mut Vec<LaidRow>,
) -> usize {
    let (shown, hidden) = visible_children(node, expanded);
    let own_rail = !shown.is_empty() || hidden.is_some();
    let n = *number;
    *number += 1;
    rows.push(LaidRow {
        target: Target::Post(node.id.clone()),
        depth,
        own_rail,
        number: Some(n),
        folded: 0,
    });
    for child in shown {
        if depth < MAX_DEPTH {
            *number = walk(child, depth + 1, expanded, number, rows);
        } else {
            rows.push(LaidRow {
                target: Target::Post(child.id.clone()),
                depth: depth + 1,
                own_rail: false,
                number: Some(*number),
                folded: 0,
            });
            *number += 1;
        }
    }
    if let Some(hidden) = hidden {
        rows.push(LaidRow {
            target: Target::Fold {
                parent: node.id.clone(),
                hidden,
            },
            depth: depth + 1,
            own_rail: false,
            number: None,
            folded: 0,
        });
    }
    *number
}

fn visible_children<'a>(
    node: &'a Node,
    expanded: &HashSet<String>,
) -> (Vec<&'a Node>, Option<usize>) {
    if expanded.contains(&node.id) || node.children.len() <= 3 {
        (node.children.iter().collect(), None)
    } else {
        let hidden = node.children.len() - 3;
        (node.children.iter().take(3).collect(), Some(hidden))
    }
}

/**
The reply trees of `posts`. The first copy of an id wins, and a parent counts when it is on the page
and is not the post itself. Posts without one are roots; then, in page order, each post no root
reaches becomes a root too, which breaks a reply cycle at its first post.
*/
fn forest(posts: &[Post], newest: bool) -> Vec<Node> {
    let mut ids = HashSet::new();
    let posts: Vec<&Post> = posts
        .iter()
        .filter(|post| ids.insert(post.id.as_str()))
        .collect();
    let map: HashMap<&str, &Post> = posts.iter().map(|p| (p.id.as_str(), *p)).collect();
    let mut child_ids: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut roots: Vec<&str> = Vec::new();
    for post in &posts {
        match post.in_reply_to.as_deref() {
            Some(parent) if map.contains_key(parent) && parent != post.id => {
                child_ids.entry(parent).or_default().push(post.id.as_str());
            }
            _ => roots.push(post.id.as_str()),
        }
    }
    let mut reached = HashSet::new();
    let mut tree: HashMap<&str, Vec<&str>> = HashMap::new();
    for root in &roots {
        mark(root, &child_ids, &mut reached, &mut tree);
    }
    for post in &posts {
        let id = post.id.as_str();
        if !reached.contains(id) {
            roots.push(id);
            mark(id, &child_ids, &mut reached, &mut tree);
        }
    }
    let mut nodes: Vec<Node> = roots
        .into_iter()
        .map(|id| build(id, 0, &map, &tree, newest))
        .collect();
    // Equal timestamps keep id order, so a redraw cannot swap two posts from the same second.
    nodes.sort_by(|a, b| b.newest.cmp(&a.newest).then(a.id.cmp(&b.id)));
    nodes
}

/// Marks everything under `root` reached and records the links that reached it in `tree`, each post once.
fn mark<'a>(
    root: &'a str,
    child_ids: &HashMap<&'a str, Vec<&'a str>>,
    reached: &mut HashSet<&'a str>,
    tree: &mut HashMap<&'a str, Vec<&'a str>>,
) {
    reached.insert(root);
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        for &child in child_ids.get(id).into_iter().flatten() {
            if reached.insert(child) {
                tree.entry(id).or_default().push(child);
                stack.push(child);
            }
        }
    }
}

/// The node for `id` at `depth`; at [`MAX_DEPTH`] every descendant becomes its direct child.
fn build(
    id: &str,
    depth: usize,
    map: &HashMap<&str, &Post>,
    tree: &HashMap<&str, Vec<&str>>,
    newest: bool,
) -> Node {
    let mut children: Vec<Node> = if depth < MAX_DEPTH {
        tree.get(id)
            .into_iter()
            .flatten()
            .map(|child| build(child, depth + 1, map, tree, newest))
            .collect()
    } else {
        let mut below = Vec::new();
        let mut stack = vec![id];
        while let Some(id) = stack.pop() {
            for &child in tree.get(id).into_iter().flatten() {
                below.push(Node {
                    id: child.to_string(),
                    children: Vec::new(),
                    newest: map.get(child).and_then(|p| p.created_at),
                });
                stack.push(child);
            }
        }
        below
    };
    children.sort_by(|a, b| {
        let ta = map.get(a.id.as_str()).and_then(|p| p.created_at);
        let tb = map.get(b.id.as_str()).and_then(|p| p.created_at);
        let time = if newest { tb.cmp(&ta) } else { ta.cmp(&tb) };
        // The id runs the same way as the time, so reversing newest-first still matches oldest-first.
        let id = if newest {
            b.id.cmp(&a.id)
        } else {
            a.id.cmp(&b.id)
        };
        time.then(id)
    });
    let own = map.get(id).and_then(|p| p.created_at);
    let newest = children.iter().filter_map(|c| c.newest).chain(own).max();
    Node {
        id: id.to_string(),
        children,
        newest,
    }
}

pub fn post_id(row: &LaidRow) -> Option<&str> {
    match &row.target {
        Target::Post(id) if !id.is_empty() => Some(id.as_str()),
        _ => None,
    }
}

pub fn selectable(rows: &[LaidRow]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, row)| !is_gap(row))
        .map(|(i, _)| i)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{demo, fixtures};

    #[test]
    fn demo_thread_hides_the_last_two_replies() {
        let now = chrono::Utc::now();
        let world = demo::world(now);
        let posts = world.posts_for_feed(&crate::model::FeedId::Merged);
        let rows = lay(&posts, &HashSet::new(), false);
        let fold = rows.iter().find_map(|r| match &r.target {
            Target::Fold { hidden, parent } if !parent.is_empty() => Some(*hidden),
            _ => None,
        });
        assert_eq!(fold, Some(2));
        let names: Vec<_> = rows
            .iter()
            .filter_map(|r| post_id(r))
            .filter_map(|id| world.posts.get(id))
            .map(|p| p.author.username.as_str())
            .collect();
        assert!(names.contains(&"mina"));
        assert!(names.contains(&"nuno"));
        assert!(names.contains(&"priya"));
        assert!(!names.contains(&"lea"));
    }

    #[test]
    fn expanding_reveals_the_tail() {
        let now = chrono::Utc::now();
        let world = demo::world(now);
        let posts = world.posts_for_feed(&crate::model::FeedId::Merged);
        let mut open = HashSet::new();
        open.insert("devnull:p2".into());
        let rows = lay(&posts, &open, false);
        let names: Vec<_> = rows
            .iter()
            .filter_map(|r| post_id(r))
            .filter_map(|id| world.posts.get(id))
            .map(|p| p.author.username.as_str())
            .collect();
        assert!(names.contains(&"lea"));
        assert!(rows.iter().all(|r| !matches!(
            &r.target,
            Target::Fold { parent, .. } if parent == "devnull:p2"
        )));
    }

    #[test]
    fn the_overview_lays_each_root_alone_with_its_reply_count() {
        let world = demo::world(chrono::Utc::now());
        let posts = world.posts_for_feed(&crate::model::FeedId::Merged);
        let rows = overview(&posts, &HashSet::new(), false);
        assert!(rows.iter().all(|r| r.depth == 0));
        assert!(!rows.iter().any(|r| matches!(r.target, Target::Fold { .. })));
        let full = lay(&posts, &HashSet::new(), false);
        let roots = |rows: &[LaidRow]| {
            rows.iter()
                .filter(|r| r.depth == 0 && matches!(r.target, Target::Post(_)))
                .count()
        };
        assert_eq!(roots(&rows), roots(&full));
        let nuno = "devnull:p2".to_string();
        let all: HashSet<String> = posts.iter().map(|p| p.id.clone()).collect();
        let open = lay(&posts, &all, false);
        let start = open
            .iter()
            .position(|r| post_id(r) == Some(nuno.as_str()))
            .unwrap();
        let below = open[start + 1..]
            .iter()
            .take_while(|r| r.depth > 0)
            .filter(|r| matches!(r.target, Target::Post(_)))
            .count();
        let row = rows
            .iter()
            .find(|r| post_id(r) == Some(nuno.as_str()))
            .unwrap();
        assert!(below >= 5, "{below}");
        assert_eq!(row.folded, below);
        assert!(full.iter().all(|r| r.folded == 0));
    }

    #[test]
    fn an_expanded_root_keeps_its_replies_in_the_overview() {
        let world = demo::world(chrono::Utc::now());
        let posts = world.posts_for_feed(&crate::model::FeedId::Merged);
        let nuno = "devnull:p2".to_string();
        let expanded: HashSet<String> = [nuno.clone()].into_iter().collect();
        let tree = |rows: &[LaidRow]| -> Vec<String> {
            let start = rows
                .iter()
                .position(|r| post_id(r) == Some(nuno.as_str()))
                .unwrap();
            rows[start..]
                .iter()
                .take_while(|r| !is_gap(r))
                .filter_map(|r| post_id(r).map(str::to_string))
                .collect()
        };
        let rows = overview(&posts, &expanded, false);
        assert_eq!(tree(&rows), tree(&lay(&posts, &expanded, false)));
        assert!(tree(&rows).len() > 1);
    }

    #[test]
    fn equal_timestamps_keep_id_order_when_the_input_is_reversed() {
        let a = fixtures::post("x:a", None, 5);
        let b = fixtures::post("x:b", None, 5);
        assert_eq!(a.created_at, b.created_at);
        let ids = |posts: &[Post]| -> Vec<String> {
            lay(posts, &HashSet::new(), false)
                .iter()
                .filter_map(post_id)
                .map(str::to_string)
                .collect()
        };
        let forward = ids(&[a.clone(), b.clone()]);
        assert_eq!(forward, ids(&[b, a]));
        assert_eq!(forward, ["x:a", "x:b"]);
    }

    #[test]
    fn newest_first_reverses_the_replies() {
        let world = demo::world(chrono::Utc::now());
        let posts = world.posts_for_feed(&crate::model::FeedId::Merged);
        let mut open = HashSet::new();
        open.insert("devnull:p2".into());
        let replies = |newest: bool| -> Vec<String> {
            lay(&posts, &open, newest)
                .iter()
                .filter(|r| r.depth == 1)
                .filter_map(post_id)
                .filter(|id| world.posts[*id].in_reply_to.as_deref() == Some("devnull:p2"))
                .map(str::to_string)
                .collect()
        };
        let oldest = replies(false);
        let mut newest = replies(true);
        assert!(oldest.len() > 1);
        newest.reverse();
        assert_eq!(newest, oldest);
    }

    #[test]
    fn malformed_reply_graphs_lay_each_post_once() {
        let graphs = [
            fixtures::cycle(),
            fixtures::self_parent(),
            fixtures::orphan(),
            fixtures::duplicates(),
            fixtures::deep_chain(5000),
        ];
        for posts in graphs {
            let ids: HashSet<String> = posts.iter().map(|post| post.id.clone()).collect();
            for newest in [false, true] {
                let rows = lay(&posts, &ids, newest);
                let mut laid: Vec<&str> = rows.iter().filter_map(post_id).collect();
                assert_eq!(laid.len(), ids.len(), "{} posts", posts.len());
                laid.sort_unstable();
                laid.dedup();
                assert_eq!(laid.len(), ids.len());
                assert!(rows.iter().all(|row| row.depth <= MAX_DEPTH + 1));
            }
        }
    }

    #[test]
    fn a_deep_chain_hangs_from_its_deepest_ancestor_in_time_order() {
        let posts = fixtures::deep_chain(200);
        let ids: HashSet<String> = posts.iter().map(|post| post.id.clone()).collect();
        let rows = lay(&posts, &ids, false);
        let deepest: Vec<&str> = rows
            .iter()
            .filter(|row| row.depth == MAX_DEPTH + 1)
            .filter_map(post_id)
            .collect();
        let expected: Vec<String> = (MAX_DEPTH + 1..200).map(|i| format!("x:p{i}")).collect();
        assert_eq!(deepest, expected);
        assert_eq!(post_id(&rows[MAX_DEPTH]), Some("x:p48"));
    }
}
