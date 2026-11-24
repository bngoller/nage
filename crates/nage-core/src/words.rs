//! The three parts of a refusal the core words (SPEC decisions 102 and 150): what was not done,
//! why, and what to do instead, kept apart. Joined, they are the one sentence the pages have
//! always shown, "<what> because <why>. <what now>", byte for byte ([`Parts::joined`]). The pages
//! are handed each error's `Display`, which keeps that sentence as a literal of its own; the words
//! tests hold the joined parts and the `Display` equal to the sentences the pages showed before.
//!
//! A command at the terminal does not print the page's sentence: it has words of its own (the
//! maintainer's pick "The terminal's own words" at PLAN M2 slice 8's pause). Its first line names
//! what the command was doing ("The book was not set up", "The user was not created", "The
//! password was not changed"), which the binary supplies; the reason and what now are the
//! refusal's twin at the terminal, [`AtTheTerminal`], which each error writes beside its page
//! parts: the reason a whole sentence, and what now naming the option to change and ending "then
//! run the command again", never "Enter" or "try again". A sentence only the terminal shows may
//! change here; one a page shows may not.
//!
//! A failure that nobody caused (a database error, a hasher that cannot hash) is not a refusal and
//! has no parts: each error's `parts` and `at_the_terminal` answer `None` for it.

#[cfg(test)]
mod tests;

/// A refusal's three parts, as the pages join them (decision 150).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    /// What was not done, without its full stop: "The user was not created".
    pub what: String,
    /// Why, as it reads after "because", without its full stop: "the username is empty".
    pub why: String,
    /// What to do instead: one or more whole sentences, each with its full stop.
    pub now: String,
}

impl Parts {
    /// The three parts of a refusal.
    pub fn new(what: impl Into<String>, why: impl Into<String>, now: impl Into<String>) -> Parts {
        Parts {
            what: what.into(),
            why: why.into(),
            now: now.into(),
        }
    }

    /// The one sentence the pages show: "<what> because <why>. <now>", exactly as the refusal
    /// read before its parts were kept apart (assumption A6 of PLAN M2 slice 8). Each error's
    /// `Display` writes the same sentence as a literal of its own; it does not call this.
    pub fn joined(&self) -> String {
        format!("{} because {}. {}", self.what, self.why, self.now)
    }
}

/// A refusal as a command says it at the terminal, beside the page's [`Parts`] (decision 150 as
/// the pause amended it): the reason after `Why:` and what to do after `What now:`, each a whole
/// sentence with its full stop. What now names the option to change (`--username`, `--by`,
/// `--data-dir`) and ends "then run the command again".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtTheTerminal {
    /// Why, a whole sentence: "The username is empty."
    pub why: String,
    /// What to do: "Give a username with --username, then run the command again."
    pub now: String,
}

impl AtTheTerminal {
    /// The reason and what to do, each a whole sentence.
    pub fn new(why: impl Into<String>, now: impl Into<String>) -> AtTheTerminal {
        AtTheTerminal {
            why: why.into(),
            now: now.into(),
        }
    }
}
