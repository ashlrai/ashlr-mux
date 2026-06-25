use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ShortcutStroke {
    pub key: String,
    #[serde(default)]
    pub command: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub option: bool,
    #[serde(default)]
    pub control: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_code: Option<u16>,
}

impl ShortcutStroke {
    pub fn has_any_modifier(&self) -> bool {
        self.command || self.shift || self.option || self.control
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StoredShortcut {
    pub first: ShortcutStroke,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub second: Option<ShortcutStroke>,
}

impl StoredShortcut {
    pub fn unbound() -> Self {
        Self {
            first: ShortcutStroke {
                key: String::new(),
                command: false,
                shift: false,
                option: false,
                control: false,
                key_code: None,
            },
            second: None,
        }
    }

    pub fn is_unbound(&self) -> bool {
        self.first.key.is_empty() && self.second.is_none()
    }

    pub fn has_chord(&self) -> bool {
        self.second.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutFocusAtom {
    SidebarFocus,
    BrowserFocus,
    MarkdownFocus,
    TerminalFocus,
}

impl ShortcutFocusAtom {
    fn from_identifier(value: &str) -> Option<Self> {
        match value {
            "sidebarFocus" => Some(Self::SidebarFocus),
            "browserFocus" => Some(Self::BrowserFocus),
            "markdownFocus" => Some(Self::MarkdownFocus),
            "terminalFocus" => Some(Self::TerminalFocus),
            _ => None,
        }
    }

    fn raw_value(&self) -> &'static str {
        match self {
            Self::SidebarFocus => "sidebarFocus",
            Self::BrowserFocus => "browserFocus",
            Self::MarkdownFocus => "markdownFocus",
            Self::TerminalFocus => "terminalFocus",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutFocusState {
    pub browser: bool,
    pub markdown: bool,
    pub sidebar: bool,
}

impl ShortcutFocusState {
    pub fn new(browser: bool, markdown: bool, sidebar: bool) -> Self {
        Self {
            browser,
            markdown,
            sidebar,
        }
    }

    pub fn terminal(&self) -> bool {
        !self.browser && !self.markdown && !self.sidebar
    }

    fn value_of(&self, atom: &ShortcutFocusAtom) -> bool {
        match atom {
            ShortcutFocusAtom::SidebarFocus => self.sidebar,
            ShortcutFocusAtom::BrowserFocus => self.browser,
            ShortcutFocusAtom::MarkdownFocus => self.markdown,
            ShortcutFocusAtom::TerminalFocus => self.terminal(),
        }
    }

    fn context(&self) -> ShortcutContext {
        let mut context = ShortcutContext::default();
        context.set_bool("sidebarFocus", self.sidebar);
        context.set_bool("browserFocus", self.browser);
        context.set_bool("markdownFocus", self.markdown);
        context.set_bool("terminalFocus", self.terminal());
        context
    }

    fn realizable_states() -> Vec<Self> {
        let mut states = Vec::new();
        for browser in [false, true] {
            for markdown in [false, true] {
                for sidebar in [false, true] {
                    if browser && markdown {
                        continue;
                    }
                    states.push(Self::new(browser, markdown, sidebar));
                }
            }
        }
        states
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutContextValue {
    Bool(bool),
    String(String),
    Int(i64),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShortcutContext {
    values: std::collections::BTreeMap<String, ShortcutContextValue>,
}

impl ShortcutContext {
    pub fn set_bool(&mut self, key: &str, value: bool) {
        self.values
            .insert(key.to_owned(), ShortcutContextValue::Bool(value));
    }

    pub fn set_string(&mut self, key: &str, value: &str) {
        self.values
            .insert(key.to_owned(), ShortcutContextValue::String(value.to_owned()));
    }

    pub fn set_int(&mut self, key: &str, value: i64) {
        self.values
            .insert(key.to_owned(), ShortcutContextValue::Int(value));
    }

    pub fn bool(&self, key: &str) -> bool {
        matches!(self.values.get(key), Some(ShortcutContextValue::Bool(true)))
    }

    pub fn string(&self, key: &str) -> Option<&str> {
        match self.values.get(key) {
            Some(ShortcutContextValue::String(value)) => Some(value),
            _ => None,
        }
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        match self.values.get(key) {
            Some(ShortcutContextValue::Int(value)) => Some(*value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ShortcutRegex {
    pub pattern: String,
    regex: Regex,
}

// `regex::Regex` is not `PartialEq`/`Eq`; the pattern fully determines the
// compiled regex, so equality is defined by the source pattern.
impl PartialEq for ShortcutRegex {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for ShortcutRegex {}

impl ShortcutRegex {
    pub fn new(pattern: &str) -> Option<Self> {
        let regex = Regex::new(pattern).ok()?;
        Some(Self {
            pattern: pattern.to_owned(),
            regex,
        })
    }

    pub fn matches(&self, value: &str) -> bool {
        self.regex.is_match(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutComparisonOperator {
    Equals,
    NotEquals,
    Matches,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    InList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutContextOperand {
    String(String),
    Int(i64),
    Regex(ShortcutRegex),
    List(Vec<ShortcutContextOperand>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutWhenClause {
    Always,
    Atom(ShortcutFocusAtom),
    Key(String),
    Compare {
        key: String,
        op: ShortcutComparisonOperator,
        operand: ShortcutContextOperand,
    },
    Not(Box<ShortcutWhenClause>),
    And(Box<ShortcutWhenClause>, Box<ShortcutWhenClause>),
    Or(Box<ShortcutWhenClause>, Box<ShortcutWhenClause>),
}

impl ShortcutWhenClause {
    pub fn parse(raw: &str) -> Option<Self> {
        if raw.trim().is_empty() {
            return Some(Self::Always);
        }
        let tokens = tokenize(raw)?;
        let mut parser = Parser { tokens, index: 0 };
        let clause = parser.parse_expression()?;
        if parser.index == parser.tokens.len() {
            Some(clause)
        } else {
            None
        }
    }

    pub fn evaluate_focus(&self, state: &ShortcutFocusState) -> bool {
        self.evaluate(&state.context())
    }

    pub fn evaluate(&self, context: &ShortcutContext) -> bool {
        match self {
            Self::Always => true,
            Self::Atom(atom) => context.bool(atom.raw_value()),
            Self::Key(name) => context.bool(name),
            Self::Compare { key, op, operand } => match op {
                ShortcutComparisonOperator::Equals => compare_equals(context, key, operand),
                ShortcutComparisonOperator::NotEquals => !compare_equals(context, key, operand),
                ShortcutComparisonOperator::Matches => {
                    matches!(operand, ShortcutContextOperand::Regex(regex) if context.string(key).is_some_and(|value| regex.matches(value)))
                }
                ShortcutComparisonOperator::LessThan => match operand {
                    ShortcutContextOperand::Int(rhs) => context.int(key).is_some_and(|lhs| lhs < *rhs),
                    _ => false,
                },
                ShortcutComparisonOperator::LessThanOrEqual => match operand {
                    ShortcutContextOperand::Int(rhs) => context.int(key).is_some_and(|lhs| lhs <= *rhs),
                    _ => false,
                },
                ShortcutComparisonOperator::GreaterThan => match operand {
                    ShortcutContextOperand::Int(rhs) => context.int(key).is_some_and(|lhs| lhs > *rhs),
                    _ => false,
                },
                ShortcutComparisonOperator::GreaterThanOrEqual => match operand {
                    ShortcutContextOperand::Int(rhs) => context.int(key).is_some_and(|lhs| lhs >= *rhs),
                    _ => false,
                },
                ShortcutComparisonOperator::InList => match operand {
                    ShortcutContextOperand::List(items) => items
                        .iter()
                        .any(|candidate| compare_equals(context, key, candidate)),
                    _ => false,
                },
            },
            Self::Not(clause) => !clause.evaluate(context),
            Self::And(lhs, rhs) => lhs.evaluate(context) && rhs.evaluate(context),
            Self::Or(lhs, rhs) => lhs.evaluate(context) || rhs.evaluate(context),
        }
    }

    pub fn can_coexist(lhs: &Self, rhs: &Self) -> bool {
        let mut uses_focus = false;
        let mut free_terms = std::collections::BTreeSet::new();
        lhs.collect_free_terms(&mut uses_focus, &mut free_terms);
        rhs.collect_free_terms(&mut uses_focus, &mut free_terms);
        let terms: Vec<String> = free_terms.into_iter().collect();
        if terms.len() > 12 {
            return true;
        }

        let focus_states = if uses_focus {
            ShortcutFocusState::realizable_states()
        } else {
            vec![ShortcutFocusState::new(false, false, false)]
        };

        for state in focus_states {
            for mask in 0..(1u32 << terms.len()) {
                let assignment = terms
                    .iter()
                    .enumerate()
                    .map(|(index, term)| (term.clone(), (mask & (1 << index)) != 0))
                    .collect::<std::collections::BTreeMap<_, _>>();
                if lhs.satisfies(&state, &assignment) && rhs.satisfies(&state, &assignment) {
                    return true;
                }
            }
        }
        false
    }

    pub fn bindings_collide(
        lhs: &Self,
        lhs_has_priority: bool,
        rhs: &Self,
        rhs_has_priority: bool,
    ) -> bool {
        if !Self::can_coexist(lhs, rhs) {
            return false;
        }
        if lhs_has_priority == rhs_has_priority {
            return true;
        }
        let (winner, loser) = if lhs_has_priority { (lhs, rhs) } else { (rhs, lhs) };
        !Self::can_coexist(loser, &Self::Not(Box::new(winner.clone())))
    }

    fn bareword_key(&self) -> Option<&str> {
        match self {
            Self::Atom(atom) => Some(atom.raw_value()),
            Self::Key(name) => Some(name),
            _ => None,
        }
    }

    fn collect_free_terms(
        &self,
        uses_focus: &mut bool,
        terms: &mut std::collections::BTreeSet<String>,
    ) {
        match self {
            Self::Always => {}
            Self::Atom(_) => *uses_focus = true,
            Self::Key(name) => {
                terms.insert(format!("key:{name}"));
            }
            Self::Compare { key, op, operand } => {
                terms.insert(format!("cmp:{key}{op:?}{operand:?}"));
            }
            Self::Not(clause) => clause.collect_free_terms(uses_focus, terms),
            Self::And(lhs, rhs) | Self::Or(lhs, rhs) => {
                lhs.collect_free_terms(uses_focus, terms);
                rhs.collect_free_terms(uses_focus, terms);
            }
        }
    }

    fn satisfies(
        &self,
        focus: &ShortcutFocusState,
        free_terms: &std::collections::BTreeMap<String, bool>,
    ) -> bool {
        match self {
            Self::Always => true,
            Self::Atom(atom) => focus.value_of(atom),
            Self::Key(name) => *free_terms.get(&format!("key:{name}")).unwrap_or(&false),
            Self::Compare { key, op, operand } => *free_terms
                .get(&format!("cmp:{key}{op:?}{operand:?}"))
                .unwrap_or(&false),
            Self::Not(clause) => !clause.satisfies(focus, free_terms),
            Self::And(lhs, rhs) => lhs.satisfies(focus, free_terms) && rhs.satisfies(focus, free_terms),
            Self::Or(lhs, rhs) => lhs.satisfies(focus, free_terms) || rhs.satisfies(focus, free_terms),
        }
    }
}

fn compare_equals(context: &ShortcutContext, key: &str, operand: &ShortcutContextOperand) -> bool {
    match operand {
        ShortcutContextOperand::String(expected) => context.string(key) == Some(expected.as_str()),
        ShortcutContextOperand::Int(expected) => context.int(key) == Some(*expected),
        ShortcutContextOperand::Regex(_) | ShortcutContextOperand::List(_) => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Not,
    And,
    Or,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Eq,
    Neq,
    MatchOp,
    Lt,
    Lte,
    Gt,
    Gte,
    Identifier(String),
    Number(i64),
    String(String),
    Regex(String),
}

fn tokenize(raw: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = raw.chars().collect();
    let mut index = 0;
    let mut tokens = Vec::new();

    while index < chars.len() {
        match chars[index] {
            ' ' | '\t' | '\n' | '\r' => index += 1,
            '(' => {
                tokens.push(Token::LParen);
                index += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                index += 1;
            }
            '[' => {
                tokens.push(Token::LBracket);
                index += 1;
            }
            ']' => {
                tokens.push(Token::RBracket);
                index += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                index += 1;
            }
            '!' => {
                index += 1;
                if chars.get(index) == Some(&'=') {
                    index += 1;
                    tokens.push(Token::Neq);
                } else {
                    tokens.push(Token::Not);
                }
            }
            '=' => {
                index += 1;
                if chars.get(index) == Some(&'=') {
                    index += 1;
                    tokens.push(Token::Eq);
                } else if chars.get(index) == Some(&'~') {
                    index += 1;
                    tokens.push(Token::MatchOp);
                } else {
                    return None;
                }
            }
            '<' => {
                index += 1;
                if chars.get(index) == Some(&'=') {
                    index += 1;
                    tokens.push(Token::Lte);
                } else {
                    tokens.push(Token::Lt);
                }
            }
            '>' => {
                index += 1;
                if chars.get(index) == Some(&'=') {
                    index += 1;
                    tokens.push(Token::Gte);
                } else {
                    tokens.push(Token::Gt);
                }
            }
            '&' => {
                index += 1;
                if chars.get(index) == Some(&'&') {
                    index += 1;
                }
                tokens.push(Token::And);
            }
            '|' => {
                index += 1;
                if chars.get(index) == Some(&'|') {
                    index += 1;
                }
                tokens.push(Token::Or);
            }
            '\'' => {
                index += 1;
                let start = index;
                while index < chars.len() && chars[index] != '\'' {
                    index += 1;
                }
                if index >= chars.len() {
                    return None;
                }
                tokens.push(Token::String(chars[start..index].iter().collect()));
                index += 1;
            }
            '/' => {
                index += 1;
                let mut pattern = String::new();
                let mut terminated = false;
                while index < chars.len() {
                    if chars[index] == '\\' && chars.get(index + 1).is_some() {
                        if chars[index + 1] == '/' {
                            pattern.push('/');
                        } else {
                            pattern.push(chars[index]);
                            pattern.push(chars[index + 1]);
                        }
                        index += 2;
                        continue;
                    }
                    if chars[index] == '/' {
                        terminated = true;
                        index += 1;
                        break;
                    }
                    pattern.push(chars[index]);
                    index += 1;
                }
                if !terminated || ShortcutRegex::new(&pattern).is_none() {
                    return None;
                }
                tokens.push(Token::Regex(pattern));
            }
            ch if ch.is_ascii_digit() => {
                let start = index;
                while index < chars.len() && chars[index].is_ascii_digit() {
                    index += 1;
                }
                tokens.push(Token::Number(
                    chars[start..index]
                        .iter()
                        .collect::<String>()
                        .parse()
                        .ok()?,
                ));
            }
            ch if ch.is_ascii_alphanumeric() || ch == '_' => {
                let start = index;
                while index < chars.len()
                    && (chars[index].is_ascii_alphanumeric() || chars[index] == '_')
                {
                    index += 1;
                }
                tokens.push(Token::Identifier(chars[start..index].iter().collect()));
            }
            _ => return None,
        }
    }

    Some(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
}

impl Parser {
    fn parse_expression(&mut self) -> Option<ShortcutWhenClause> {
        self.parse_or()
    }

    fn parse_or(&mut self) -> Option<ShortcutWhenClause> {
        let mut lhs = self.parse_and()?;
        while matches!(self.tokens.get(self.index), Some(Token::Or)) {
            self.index += 1;
            let rhs = self.parse_and()?;
            lhs = ShortcutWhenClause::Or(Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }

    fn parse_and(&mut self) -> Option<ShortcutWhenClause> {
        let mut lhs = self.parse_comparison()?;
        while matches!(self.tokens.get(self.index), Some(Token::And)) {
            self.index += 1;
            let rhs = self.parse_comparison()?;
            lhs = ShortcutWhenClause::And(Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }

    fn parse_comparison(&mut self) -> Option<ShortcutWhenClause> {
        let lhs = self.parse_unary()?;
        let op = match self.tokens.get(self.index) {
            Some(Token::Eq) => Some(ShortcutComparisonOperator::Equals),
            Some(Token::Neq) => Some(ShortcutComparisonOperator::NotEquals),
            Some(Token::MatchOp) => Some(ShortcutComparisonOperator::Matches),
            Some(Token::Lt) => Some(ShortcutComparisonOperator::LessThan),
            Some(Token::Lte) => Some(ShortcutComparisonOperator::LessThanOrEqual),
            Some(Token::Gt) => Some(ShortcutComparisonOperator::GreaterThan),
            Some(Token::Gte) => Some(ShortcutComparisonOperator::GreaterThanOrEqual),
            Some(Token::Identifier(value)) if value == "in" => Some(ShortcutComparisonOperator::InList),
            _ => None,
        };
        let Some(op) = op else { return Some(lhs) };
        let key = lhs.bareword_key()?.to_owned();
        self.index += 1;
        let operand = self.parse_operand(op)?;
        if matches!(
            (&op, &operand),
            (
                ShortcutComparisonOperator::Equals | ShortcutComparisonOperator::NotEquals,
                ShortcutContextOperand::String(value)
            ) if value == "true" || value == "false"
        ) {
            let wants_key_true = matches!((&op, &operand),
                (ShortcutComparisonOperator::Equals, ShortcutContextOperand::String(value)) if value == "true")
                || matches!((&op, &operand),
                (ShortcutComparisonOperator::NotEquals, ShortcutContextOperand::String(value)) if value == "false");
            return Some(if wants_key_true {
                lhs
            } else {
                ShortcutWhenClause::Not(Box::new(lhs))
            });
        }
        Some(ShortcutWhenClause::Compare { key, op, operand })
    }

    fn parse_unary(&mut self) -> Option<ShortcutWhenClause> {
        if matches!(self.tokens.get(self.index), Some(Token::Not)) {
            self.index += 1;
            return Some(ShortcutWhenClause::Not(Box::new(self.parse_unary()?)));
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Option<ShortcutWhenClause> {
        match self.tokens.get(self.index)?.clone() {
            Token::LParen => {
                self.index += 1;
                let inner = self.parse_expression()?;
                if !matches!(self.tokens.get(self.index), Some(Token::RParen)) {
                    return None;
                }
                self.index += 1;
                Some(inner)
            }
            Token::Identifier(name) => {
                self.index += 1;
                if name == "true" {
                    Some(ShortcutWhenClause::Always)
                } else if name == "false" {
                    Some(ShortcutWhenClause::Not(Box::new(ShortcutWhenClause::Always)))
                } else if let Some(atom) = ShortcutFocusAtom::from_identifier(&name) {
                    Some(ShortcutWhenClause::Atom(atom))
                } else {
                    Some(ShortcutWhenClause::Key(name))
                }
            }
            _ => None,
        }
    }

    fn parse_operand(&mut self, op: ShortcutComparisonOperator) -> Option<ShortcutContextOperand> {
        match op {
            ShortcutComparisonOperator::Matches => match self.tokens.get(self.index)?.clone() {
                Token::Regex(pattern) => {
                    self.index += 1;
                    Some(ShortcutContextOperand::Regex(ShortcutRegex::new(&pattern)?))
                }
                _ => None,
            },
            ShortcutComparisonOperator::LessThan
            | ShortcutComparisonOperator::LessThanOrEqual
            | ShortcutComparisonOperator::GreaterThan
            | ShortcutComparisonOperator::GreaterThanOrEqual => match self.tokens.get(self.index)?.clone() {
                Token::Number(value) => {
                    self.index += 1;
                    Some(ShortcutContextOperand::Int(value))
                }
                _ => None,
            },
            ShortcutComparisonOperator::InList => self.parse_list_operand(),
            ShortcutComparisonOperator::Equals | ShortcutComparisonOperator::NotEquals => {
                match self.tokens.get(self.index)?.clone() {
                    Token::Number(value) => {
                        self.index += 1;
                        Some(ShortcutContextOperand::Int(value))
                    }
                    Token::String(value) | Token::Identifier(value) => {
                        self.index += 1;
                        Some(ShortcutContextOperand::String(value))
                    }
                    _ => None,
                }
            }
        }
    }

    fn parse_list_operand(&mut self) -> Option<ShortcutContextOperand> {
        if !matches!(self.tokens.get(self.index), Some(Token::LBracket)) {
            return None;
        }
        self.index += 1;
        let mut items = Vec::new();
        if matches!(self.tokens.get(self.index), Some(Token::RBracket)) {
            self.index += 1;
            return Some(ShortcutContextOperand::List(items));
        }
        loop {
            match self.tokens.get(self.index)?.clone() {
                Token::Number(value) => items.push(ShortcutContextOperand::Int(value)),
                Token::String(value) | Token::Identifier(value) => {
                    items.push(ShortcutContextOperand::String(value))
                }
                _ => return None,
            }
            self.index += 1;
            match self.tokens.get(self.index) {
                Some(Token::Comma) => self.index += 1,
                Some(Token::RBracket) => {
                    self.index += 1;
                    return Some(ShortcutContextOperand::List(items));
                }
                _ => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(browser: bool, markdown: bool, sidebar: bool) -> ShortcutFocusState {
        ShortcutFocusState::new(browser, markdown, sidebar)
    }

    fn sample_context() -> ShortcutContext {
        let mut context = ShortcutContext::default();
        context.set_bool("commandPaletteVisible", true);
        context.set_string("sidebarMode", "find");
        context.set_int("paneCount", 2);
        context
    }

    #[test]
    fn empty_clause_parses_to_always() {
        assert_eq!(ShortcutWhenClause::parse(""), Some(ShortcutWhenClause::Always));
        assert_eq!(ShortcutWhenClause::parse("   "), Some(ShortcutWhenClause::Always));
    }

    #[test]
    fn parses_and_or_with_precedence() {
        assert_eq!(
            ShortcutWhenClause::parse("terminalFocus || browserFocus && markdownFocus"),
            Some(ShortcutWhenClause::Or(
                Box::new(ShortcutWhenClause::Atom(ShortcutFocusAtom::TerminalFocus)),
                Box::new(ShortcutWhenClause::And(
                    Box::new(ShortcutWhenClause::Atom(ShortcutFocusAtom::BrowserFocus)),
                    Box::new(ShortcutWhenClause::Atom(ShortcutFocusAtom::MarkdownFocus)),
                )),
            ))
        );
    }

    #[test]
    fn parses_boolean_literals_and_unknown_keys() {
        assert_eq!(ShortcutWhenClause::parse("true"), Some(ShortcutWhenClause::Always));
        assert_eq!(
            ShortcutWhenClause::parse("false"),
            Some(ShortcutWhenClause::Not(Box::new(ShortcutWhenClause::Always)))
        );
        assert_eq!(
            ShortcutWhenClause::parse("commandPaletteVisible"),
            Some(ShortcutWhenClause::Key("commandPaletteVisible".into()))
        );
    }

    #[test]
    fn evaluates_against_context() {
        let context = sample_context();
        assert!(ShortcutWhenClause::parse("commandPaletteVisible")
            .expect("clause")
            .evaluate(&context));
        assert!(ShortcutWhenClause::parse("paneCount > 1")
            .expect("clause")
            .evaluate(&context));
        assert!(ShortcutWhenClause::parse("sidebarMode =~ /^fi/")
            .expect("clause")
            .evaluate(&context));
    }

    #[test]
    fn focus_overload_matches_terminal_behavior() {
        assert!(ShortcutWhenClause::Atom(ShortcutFocusAtom::TerminalFocus).evaluate_focus(&state(
            false, false, false
        )));
        assert!(!ShortcutWhenClause::Atom(ShortcutFocusAtom::SidebarFocus).evaluate_focus(&state(
            false, false, false
        )));
    }

    #[test]
    fn coexist_and_priority_match_swift_rules() {
        let sidebar = ShortcutWhenClause::Atom(ShortcutFocusAtom::SidebarFocus);
        let workspace = ShortcutWhenClause::parse("!sidebarFocus").expect("workspace");
        assert!(!ShortcutWhenClause::can_coexist(&workspace, &sidebar));
        assert!(!ShortcutWhenClause::bindings_collide(
            &ShortcutWhenClause::Always,
            false,
            &sidebar,
            true,
        ));
    }

    #[test]
    fn stored_shortcut_helpers_match_wire_shape() {
        let shortcut = StoredShortcut::unbound();
        assert!(shortcut.is_unbound());
        assert!(!shortcut.has_chord());
        let json = serde_json::to_value(&shortcut).expect("serialize");
        assert!(json.get("first").is_some());
    }
}
