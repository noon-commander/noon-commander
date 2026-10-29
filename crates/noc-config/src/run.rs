use noc_vars::{Template, VarsError};
use serde::{Deserialize, Serialize};

/// What runs a command of the command line: a program and its arguments, with variables in
/// them, after which the command comes as one more argument (ADR 0054). `shell.run`, and the
/// `run` of a host in `hosts.toml`. Never empty.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct Run(Vec<Template>);

/// Why a list of words is not a [`Run`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    #[error("an empty list names no program")]
    Empty,
    #[error(transparent)]
    Word(#[from] VarsError),
}

impl Run {
    /// The program, then its arguments.
    pub fn words(&self) -> &[Template] {
        &self.0
    }
}

impl Default for Run {
    /// `$SHELL -i -c`: the user's shell, interactive, so that it reads its rc files and has the
    /// aliases and functions they define.
    fn default() -> Self {
        // Each of these parses; a literal would only stand in for one that did not.
        let word = |text: &str| Template::parse(text).unwrap_or_else(|_| Template::literal(text));
        Self(vec![word("$SHELL"), word("-i"), word("-c")])
    }
}

impl TryFrom<Vec<String>> for Run {
    type Error = RunError;

    fn try_from(words: Vec<String>) -> Result<Self, Self::Error> {
        if words.is_empty() {
            return Err(RunError::Empty);
        }
        let words = words
            .iter()
            .map(|word| Template::parse(word))
            .collect::<Result<_, _>>()?;
        Ok(Self(words))
    }
}

impl From<Run> for Vec<String> {
    fn from(run: Run) -> Self {
        run.0.iter().map(Template::to_string).collect()
    }
}

#[cfg(test)]
mod tests {
    use noc_vars::Piece;

    use super::*;

    fn run(words: &[&str]) -> Result<Run, RunError> {
        Run::try_from(
            words
                .iter()
                .map(|&word| word.to_owned())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn the_default_is_the_users_shell_interactive() {
        let default = Run::default();
        assert_eq!(default, run(&["$SHELL", "-i", "-c"]).unwrap());
        assert_eq!(
            default.words()[0].pieces(),
            [Piece::Var("SHELL".to_owned())]
        );
        assert_eq!(Vec::<String>::from(default), ["$SHELL", "-i", "-c"]);
    }

    #[test]
    fn needs_a_program_and_words_that_read() {
        assert_eq!(run(&[]), Err(RunError::Empty));
        assert_eq!(
            run(&["${SHELL", "-c"]),
            Err(RunError::Word(VarsError::Unclosed("${SHELL".to_owned())))
        );
        let words = ["/bin/bash", "--rcfile", "${HOME}/.noc-rc", "-ic"];
        let read = run(&words).unwrap();
        assert_eq!(Vec::<String>::from(read), words);
    }
}
