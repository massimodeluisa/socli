//! The `socli>` command line. Same grammar in the reader and the scrollback prompt.

use crate::model::{Scope, Section};
use crate::net::{FeedKind, Network};
use crate::theme::Scheme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd {
    Help,
    Quit,
    Refresh,
    More,
    Section(Section),
    Tab(FeedKind),
    Scope(Scope),
    Search(String),
    User(String),
    Thread(String),
    Open { n: Option<usize> },
    View { n: Option<usize> },
    Avatar { n: Option<usize> },
    Json { n: Option<usize> },
    Share { n: Option<usize> },
    Post(String),
    Reply { n: Option<usize>, text: String },
    Repost { n: Option<usize> },
    Unrepost { n: Option<usize> },
    Bookmark { n: Option<usize> },
    Unbookmark { n: Option<usize> },
    Like { n: Option<usize> },
    Unlike { n: Option<usize> },
    Follow(String),
    Unfollow(String),
    Delete { n: Option<usize> },
    Quote { n: Option<usize>, text: String },
    Expand,
    Back,
    SelectDelta(i32),
    SelectFirst,
    SelectLast,
    Filter(Option<String>),
    Theme(Option<Scheme>),
}

pub const HELP: &str = "\
next  prev                     move the selection
expand                         open the thread, or show folded replies
back                           return to the last view, or close the thread or filter
more  refresh                  load the next page, or reload this one
home  notifications            go to a section
search [words]                 go to Search, or search for words
net <slug|all>                 show one network, or all of them
theme [name]                   pick a theme, or switch to one
following  foryou  latest      pick a home tab
bookmarks  trending            pick a home tab
discover  saved  mine          pick a home tab
user @handle                   posts from one account
thread <id>                    open a conversation
post <text>                    publish a post
reply [n] <text>               reply to post n, or to the selection
quote [n] <text>               quote post n
like [n]  unlike [n]           like a post, or undo it
repost [n]  unrepost [n]       repost, or undo it
bookmark [n]  unbookmark [n]   save a post, or remove it
follow @handle                 follow an account
unfollow @handle               unfollow an account
delete [n]                     delete one of your posts
open [n]  view [n]             open the link, or show the image
share [n]                      copy the post's link
avatar [n]  json [n]           show the avatar, or print the post as JSON
filter <words>  clear          filter this page, or clear the filter
help  quit                     this list, or leave
";

/// Every command the `:` line suggests: name, arguments, and what it does.
pub const COMMANDS: &[(&str, &str, &str)] = &[
    ("next", "", "move the selection down"),
    ("prev", "", "move the selection up"),
    ("expand", "", "open the thread, or show folded replies"),
    ("back", "", "return to the last view"),
    ("more", "", "load the next page"),
    ("refresh", "", "reload this page"),
    ("home", "", "go to Home"),
    ("notifications", "", "go to Notifications"),
    ("net", "<slug|all>", "show one network, or all of them"),
    ("theme", "[name]", "pick a theme, or switch to one"),
    ("following", "", "feed tab Following"),
    ("foryou", "", "feed tab For you"),
    ("latest", "", "feed tab Latest"),
    ("bookmarks", "", "feed tab Bookmarks"),
    ("trending", "", "feed tab Trending"),
    ("discover", "", "feed tab Discover"),
    ("saved", "", "your saved posts"),
    ("mine", "", "your own posts"),
    ("search", "[words]", "go to Search, or search for words"),
    ("user", "@handle", "posts from one account"),
    ("thread", "<id>", "open a conversation"),
    ("post", "<text>", "publish a post"),
    (
        "reply",
        "[n] <text>",
        "reply to post n, or to the selection",
    ),
    ("quote", "[n] <text>", "quote post n, or the selection"),
    ("like", "[n]", "like a post"),
    ("unlike", "[n]", "undo a like"),
    ("repost", "[n]", "repost a post"),
    ("unrepost", "[n]", "undo a repost"),
    ("bookmark", "[n]", "save a post"),
    ("unbookmark", "[n]", "remove a saved post"),
    ("share", "[n]", "copy a post's link"),
    ("follow", "@handle", "follow an account"),
    ("unfollow", "@handle", "unfollow an account"),
    ("delete", "[n]", "delete one of your posts"),
    ("open", "[n]", "open the link in the browser"),
    ("view", "[n]", "show the images"),
    ("avatar", "[n]", "show the avatar"),
    ("json", "[n]", "print the post as JSON"),
    ("filter", "<words>", "filter this page"),
    ("clear", "", "clear the filter"),
    ("help", "", "list the keys"),
    ("quit", "", "leave socli"),
];

pub fn parse(input: &str) -> Result<Cmd, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("type a command, or ? for keys".into());
    }
    let parts = split_first(input);
    let head = parts.0.to_ascii_lowercase();
    let rest = parts.1;
    match head.as_str() {
        "help" | "?" | "h" => Ok(Cmd::Help),
        "q" | "quit" | "exit" => Ok(Cmd::Quit),
        "refresh" | "reload" => Ok(Cmd::Refresh),
        "more" => Ok(Cmd::More),
        "back" => Ok(Cmd::Back),
        "log" | "home" => Ok(Cmd::Section(Section::Home)),
        "notifications" | "notif" | "mentions" => Ok(Cmd::Section(Section::Notifications)),
        "net" | "network" => match rest.trim().to_ascii_lowercase().as_str() {
            "all" => Ok(Cmd::Scope(Scope::All)),
            slug => slug
                .parse::<Network>()
                .map(|net| Cmd::Scope(Scope::One(net)))
                .map_err(|_| "net needs x, bluesky, devnull, threads, mastodon or all".to_string()),
        },
        "theme" | "themes" => match rest.trim() {
            "" => Ok(Cmd::Theme(None)),
            name => Scheme::parse(name)
                .map(|scheme| Cmd::Theme(Some(scheme)))
                .ok_or_else(|| format!("\"{rest}\" is not a theme. :theme opens the list")),
        },
        "following" => Ok(Cmd::Tab(FeedKind::Following)),
        "foryou" | "for-you" => Ok(Cmd::Tab(FeedKind::ForYou)),
        "latest" => Ok(Cmd::Tab(FeedKind::Latest)),
        "bookmarks" | "saved" => Ok(Cmd::Tab(FeedKind::Bookmarks)),
        "trending" | "trends" => Ok(Cmd::Tab(FeedKind::Trending)),
        "discover" => Ok(Cmd::Tab(FeedKind::Discover)),
        "mine" => Ok(Cmd::Tab(FeedKind::Mine)),
        "open" => Ok(Cmd::Open {
            n: optional_index(&head, rest)?,
        }),
        "view" => Ok(Cmd::View {
            n: optional_index(&head, rest)?,
        }),
        "share" | "s" => Ok(Cmd::Share {
            n: optional_index(&head, rest)?,
        }),
        "avatar" => Ok(Cmd::Avatar {
            n: optional_index(&head, rest)?,
        }),
        "json" | "inspect" => Ok(Cmd::Json {
            n: optional_index(&head, rest)?,
        }),
        "expand" => Ok(Cmd::Expand),
        "next" | "j" => Ok(Cmd::SelectDelta(1)),
        "prev" | "k" => Ok(Cmd::SelectDelta(-1)),
        "search" | "f" | "/" | "remotes" | "remote" => {
            let q = rest.trim();
            if q.is_empty() {
                Ok(Cmd::Section(Section::Search))
            } else {
                Ok(Cmd::Search(q.to_string()))
            }
        }
        "user" | "u" => {
            let handle = clean_handle(rest);
            if handle.is_empty() {
                Err("user needs a @handle".into())
            } else {
                Ok(Cmd::User(handle))
            }
        }
        "thread" => {
            let id = rest.trim();
            if id.is_empty() {
                Err("thread needs a post id".into())
            } else {
                Ok(Cmd::Thread(id.to_string()))
            }
        }
        "post" | "n" => {
            let text = rest.trim();
            if text.is_empty() {
                Err("post needs text".into())
            } else {
                Ok(Cmd::Post(text.to_string()))
            }
        }
        "reply" | "r" => {
            let (n, text) = split_index(rest);
            if text.is_empty() {
                Err("reply needs text".into())
            } else {
                Ok(Cmd::Reply {
                    n,
                    text: text.to_string(),
                })
            }
        }
        "quote" => {
            let (n, text) = split_index(rest);
            if text.is_empty() {
                Err("quote needs text".into())
            } else {
                Ok(Cmd::Quote {
                    n,
                    text: text.to_string(),
                })
            }
        }
        "repost" | "rt" | "t" => Ok(Cmd::Repost {
            n: optional_index(&head, rest)?,
        }),
        "unrepost" => Ok(Cmd::Unrepost {
            n: optional_index(&head, rest)?,
        }),
        "bookmark" => Ok(Cmd::Bookmark {
            n: optional_index(&head, rest)?,
        }),
        "unbookmark" => Ok(Cmd::Unbookmark {
            n: optional_index(&head, rest)?,
        }),
        "like" | "l" | "fav" => Ok(Cmd::Like {
            n: optional_index(&head, rest)?,
        }),
        "unlike" => Ok(Cmd::Unlike {
            n: optional_index(&head, rest)?,
        }),
        "follow" => {
            let handle = clean_handle(rest);
            if handle.is_empty() {
                Err("follow needs a @handle".into())
            } else {
                Ok(Cmd::Follow(handle))
            }
        }
        "unfollow" => {
            let handle = clean_handle(rest);
            if handle.is_empty() {
                Err("unfollow needs a @handle".into())
            } else {
                Ok(Cmd::Unfollow(handle))
            }
        }
        "delete" | "del" => Ok(Cmd::Delete {
            n: optional_index(&head, rest)?,
        }),
        "filter" => {
            let q = rest.trim();
            if q.is_empty() {
                Ok(Cmd::Filter(None))
            } else {
                Ok(Cmd::Filter(Some(q.to_string())))
            }
        }
        "clear" => Ok(Cmd::Filter(None)),
        other => Err(format!("unknown command `{other}` · ? lists keys")),
    }
}

fn split_first(input: &str) -> (&str, &str) {
    input
        .split_once(char::is_whitespace)
        .map(|(a, b)| (a, b.trim()))
        .unwrap_or((input, ""))
}

/**
An empty rest means the selection. A present token must be exactly one `usize`.
Anything else is an error, so a bad index cannot fall through to the selected row.
*/
fn optional_index(command: &str, rest: &str) -> Result<Option<usize>, String> {
    let mut tokens = rest.split_whitespace();
    let Some(token) = tokens.next() else {
        return Ok(None);
    };
    if tokens.next().is_some() {
        return Err(format!("{command} takes one post number"));
    }
    token
        .parse::<usize>()
        .map(Some)
        .map_err(|_| format!("{command} needs a post number, or nothing to use the selection"))
}

/// `2 hello there` -> (Some(2), "hello there"); `hello` -> (None, "hello").
fn split_index(rest: &str) -> (Option<usize>, &str) {
    let rest = rest.trim();
    let Some((first, tail)) = rest.split_once(char::is_whitespace) else {
        return if rest.parse::<usize>().is_ok() {
            (rest.parse().ok(), "")
        } else {
            (None, rest)
        };
    };
    if let Ok(n) = first.parse::<usize>() {
        (Some(n), tail.trim())
    } else {
        (None, rest)
    }
}

pub fn clean_handle(raw: &str) -> String {
    raw.trim().trim_start_matches('@').trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_with_index() {
        assert_eq!(
            parse("reply 2 ship it").unwrap(),
            Cmd::Reply {
                n: Some(2),
                text: "ship it".into()
            }
        );
    }

    #[test]
    fn reply_uses_selection_when_unnumbered() {
        assert_eq!(
            parse("reply ship it").unwrap(),
            Cmd::Reply {
                n: None,
                text: "ship it".into()
            }
        );
    }

    #[test]
    fn theme_opens_the_list_or_names_one() {
        use crate::theme::{Scheme, Variant};
        assert_eq!(parse("theme").unwrap(), Cmd::Theme(None));
        assert_eq!(parse("themes").unwrap(), Cmd::Theme(None));
        let nord = Scheme::parse("nord").unwrap();
        assert!(matches!(nord, Scheme::Named(_, Variant::Follow)));
        assert_eq!(parse("theme nord").unwrap(), Cmd::Theme(Some(nord)));
        assert_eq!(
            parse("theme nope").unwrap_err(),
            "\"nope\" is not a theme. :theme opens the list"
        );
    }

    #[test]
    fn net_picks_a_scope() {
        assert_eq!(
            parse("net bluesky").unwrap(),
            Cmd::Scope(Scope::One(Network::Bluesky))
        );
        assert_eq!(parse("net all").unwrap(), Cmd::Scope(Scope::All));
        assert!(parse("net myspace").is_err());
        assert_eq!(parse("saved").unwrap(), Cmd::Tab(FeedKind::Bookmarks));
    }

    #[test]
    fn search_letters_match_the_reader_keys() {
        for head in ["search", "f", "/"] {
            assert_eq!(
                parse(&format!("{head} rust tui")).unwrap(),
                Cmd::Search("rust tui".into()),
                "{head}"
            );
            assert_eq!(
                parse(head).unwrap(),
                Cmd::Section(Section::Search),
                "{head}"
            );
        }
    }

    #[test]
    fn like_letters_match_the_reader_keys() {
        for head in ["like", "l", "fav"] {
            assert_eq!(parse(head).unwrap(), Cmd::Like { n: None }, "{head}");
            assert_eq!(
                parse(&format!("{head} 3")).unwrap(),
                Cmd::Like { n: Some(3) },
                "{head}"
            );
        }
    }

    #[test]
    fn share_letters_match_the_reader_keys() {
        for head in ["share", "s"] {
            assert_eq!(parse(head).unwrap(), Cmd::Share { n: None }, "{head}");
            assert_eq!(
                parse(&format!("{head} 2")).unwrap(),
                Cmd::Share { n: Some(2) },
                "{head}"
            );
        }
    }

    #[test]
    fn a_bad_post_number_is_rejected() {
        assert_eq!(parse("delete").unwrap(), Cmd::Delete { n: None });
        assert_eq!(parse("delete 2").unwrap(), Cmd::Delete { n: Some(2) });
        assert_eq!(
            parse("delete xyz").unwrap_err(),
            "delete needs a post number, or nothing to use the selection"
        );
        assert_eq!(
            parse("delete 99999999999999999999").unwrap_err(),
            "delete needs a post number, or nothing to use the selection"
        );
        assert_eq!(
            parse("delete 1 extra").unwrap_err(),
            "delete takes one post number"
        );
        assert_eq!(
            parse("like xyz").unwrap_err(),
            "like needs a post number, or nothing to use the selection"
        );
        assert_eq!(
            parse("bookmark 1 extra").unwrap_err(),
            "bookmark takes one post number"
        );
        assert_eq!(
            parse("repost nope").unwrap_err(),
            "repost needs a post number, or nothing to use the selection"
        );
    }

    #[test]
    fn strips_at_sign() {
        assert_eq!(parse("follow @Mina").unwrap(), Cmd::Follow("Mina".into()));
    }

    #[test]
    fn every_listed_command_parses() {
        for (name, args, _) in COMMANDS {
            let sample = match *args {
                "" | "[n]" | "[words]" | "[name]" => "",
                "@handle" => "@ada",
                "<words>" | "<text>" | "[n] <text>" => "hello",
                "<id>" => "x:1",
                "<slug|all>" => "all",
                other => panic!("no sample for {other}"),
            };
            let input = format!("{name} {sample}");
            assert!(parse(&input).is_ok(), "{input}");
        }
    }
}
