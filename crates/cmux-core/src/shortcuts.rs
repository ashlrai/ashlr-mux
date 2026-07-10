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
    // Wire key is `keyCode` (camelCase) to match the authoritative macOS Swift
    // codec (`ShortcutStroke` in CmuxSettings uses synthesized Codable, so the
    // on-disk `cmux.json` shortcut bindings carry `keyCode`). The Rust field stays
    // snake_case idiomatically; only the serialized key is renamed.
    #[serde(rename = "keyCode", default, skip_serializing_if = "Option::is_none")]
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
        self.values.insert(
            key.to_owned(),
            ShortcutContextValue::String(value.to_owned()),
        );
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
                    ShortcutContextOperand::Int(rhs) => {
                        context.int(key).is_some_and(|lhs| lhs < *rhs)
                    }
                    _ => false,
                },
                ShortcutComparisonOperator::LessThanOrEqual => match operand {
                    ShortcutContextOperand::Int(rhs) => {
                        context.int(key).is_some_and(|lhs| lhs <= *rhs)
                    }
                    _ => false,
                },
                ShortcutComparisonOperator::GreaterThan => match operand {
                    ShortcutContextOperand::Int(rhs) => {
                        context.int(key).is_some_and(|lhs| lhs > *rhs)
                    }
                    _ => false,
                },
                ShortcutComparisonOperator::GreaterThanOrEqual => match operand {
                    ShortcutContextOperand::Int(rhs) => {
                        context.int(key).is_some_and(|lhs| lhs >= *rhs)
                    }
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
        let (winner, loser) = if lhs_has_priority {
            (lhs, rhs)
        } else {
            (rhs, lhs)
        };
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
            Self::And(lhs, rhs) => {
                lhs.satisfies(focus, free_terms) && rhs.satisfies(focus, free_terms)
            }
            Self::Or(lhs, rhs) => {
                lhs.satisfies(focus, free_terms) || rhs.satisfies(focus, free_terms)
            }
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
            Some(Token::Identifier(value)) if value == "in" => {
                Some(ShortcutComparisonOperator::InList)
            }
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
                    Some(ShortcutWhenClause::Not(Box::new(
                        ShortcutWhenClause::Always,
                    )))
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
            | ShortcutComparisonOperator::GreaterThanOrEqual => {
                match self.tokens.get(self.index)?.clone() {
                    Token::Number(value) => {
                        self.index += 1;
                        Some(ShortcutContextOperand::Int(value))
                    }
                    _ => None,
                }
            }
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
        assert_eq!(
            ShortcutWhenClause::parse(""),
            Some(ShortcutWhenClause::Always)
        );
        assert_eq!(
            ShortcutWhenClause::parse("   "),
            Some(ShortcutWhenClause::Always)
        );
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
        assert_eq!(
            ShortcutWhenClause::parse("true"),
            Some(ShortcutWhenClause::Always)
        );
        assert_eq!(
            ShortcutWhenClause::parse("false"),
            Some(ShortcutWhenClause::Not(Box::new(
                ShortcutWhenClause::Always
            )))
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
        assert!(ShortcutWhenClause::Atom(ShortcutFocusAtom::TerminalFocus)
            .evaluate_focus(&state(false, false, false)));
        assert!(!ShortcutWhenClause::Atom(ShortcutFocusAtom::SidebarFocus)
            .evaluate_focus(&state(false, false, false)));
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

// ===========================================================================
// APPENDED: keyboard-shortcut MODEL logic (lane `shortcut-model`).
//
// Ports the remaining pure logic from the canonical macOS Swift sources:
//   - Sources/KeyboardShortcutSettings.swift
//   - Packages/macOS/CmuxSettings/.../ShortcutDisplayFormatter.swift
//   - Packages/macOS/CmuxSettings/.../ShortcutAction.swift
//
// Reuses (never redefines) the already-ported `ShortcutStroke`,
// `StoredShortcut`, and the `ShortcutWhenClause` stack above, and the
// `Action` catalog from `shortcuts_action.rs`.
//
// macOS-only pieces (NSEvent/SwiftUI/Carbon/KeyboardLayout, SystemWideHotkey
// registration and its reserved-hotkey scan) are intentionally EXCLUDED; where
// a Swift branch depends on them, a `// DIVERGENCE:` comment records the gap
// and the pure fallback behavior.
// ===========================================================================

use crate::shortcuts_action::Action;

// ---------------------------------------------------------------------------
// (1) CONFIG-STRING CODEC
// Swift refs: KeyboardShortcutSettings.swift:2384-2513 (ShortcutStroke codec),
// :2516-2553 (StoredShortcut codec).
// ---------------------------------------------------------------------------

/// Parse a single config key token (the part after the last `+`).
///
/// Mirrors `ShortcutStroke.parseConfigKeyToken` (Swift :2444-2513). The raw
/// (untrimmed) token is passed in so a bare single space maps to `"space"`.
pub fn parse_config_key_token(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        // A lone space is the space key; any other all-whitespace token is invalid.
        return if raw == " " {
            Some("space".to_string())
        } else {
            None
        };
    }

    let lowered = trimmed.to_lowercase();
    let mapped = match lowered.as_str() {
        "left" | "arrowleft" | "leftarrow" | "←" => "←",
        "right" | "arrowright" | "rightarrow" | "→" => "→",
        "up" | "arrowup" | "uparrow" | "↑" => "↑",
        "down" | "arrowdown" | "downarrow" | "↓" => "↓",
        "tab" => "\t",
        "return" | "enter" | "↩" => "\r",
        "space" | "spacebar" | "<space>" => "space",
        "comma" => ",",
        "period" | "dot" => ".",
        "slash" => "/",
        "backslash" => "\\",
        "semicolon" => ";",
        "quote" | "apostrophe" => "'",
        "backtick" | "grave" => "`",
        "minus" | "hyphen" => "-",
        "plus" | "equals" => "=",
        "leftbracket" | "openbracket" => "[",
        "rightbracket" | "closebracket" => "]",
        "volumeup" | "mediavolumeup" | "media.volumeup" => "media.volumeUp",
        "volumedown" | "mediavolumedown" | "media.volumedown" => "media.volumeDown",
        "brightnessup" | "mediabrightnessup" | "media.brightnessup" => "media.brightnessUp",
        "brightnessdown" | "mediabrightnessdown" | "media.brightnessdown" => "media.brightnessDown",
        "mute" | "mediamute" | "media.mute" => "media.mute",
        "playpause" | "mediaplaypause" | "media.playpause" => "media.playPause",
        "nexttrack" | "medianext" | "media.next" | "media.nexttrack" => "media.next",
        "previoustrack" | "mediaprevious" | "media.previous" | "media.previoustrack" => {
            "media.previous"
        }
        _ => {
            // f1..f20 named function keys.
            if let Some(rest) = lowered.strip_prefix('f') {
                if let Ok(number) = rest.parse::<u32>() {
                    if (1..=20).contains(&number) {
                        return Some(format!("f{number}"));
                    }
                }
            }
            // Otherwise only a single lowercased character is a valid key token.
            if lowered.chars().count() == 1 {
                return Some(lowered);
            }
            return None;
        }
    };
    Some(mapped.to_string())
}

impl ShortcutStroke {
    /// Whether this stroke carries a "primary" (non-shift) modifier.
    /// Swift `ShortcutStroke.hasPrimaryModifier` (:1556-1558).
    pub fn has_primary_modifier(&self) -> bool {
        self.command || self.option || self.control
    }

    /// Parse a single stroke from a `mod+mod+key` config string.
    /// Swift `ShortcutStroke.parseConfig(_:)` (:2385-2423).
    pub fn parse_config(raw_value: &str) -> Option<ShortcutStroke> {
        if raw_value.is_empty() {
            return None;
        }

        // Swift: split(separator: "+", omittingEmptySubsequences: false).
        let raw_parts: Vec<&str> = raw_value.split('+').collect();
        let parts: Vec<String> = raw_parts.iter().map(|p| p.trim().to_string()).collect();
        let last_raw_part = *raw_parts.last()?;
        if parts.is_empty() || last_raw_part.is_empty() {
            return None;
        }

        let mut command = false;
        let mut shift = false;
        let mut option = false;
        let mut control = false;

        for modifier in &parts[..parts.len() - 1] {
            match modifier.to_lowercase().as_str() {
                "cmd" | "command" | "⌘" => command = true,
                "shift" | "⇧" => shift = true,
                "opt" | "option" | "alt" | "⌥" => option = true,
                "ctrl" | "control" | "ctl" | "⌃" => control = true,
                _ => return None,
            }
        }

        let key = parse_config_key_token(last_raw_part)?;
        Some(ShortcutStroke {
            key,
            command,
            shift,
            option,
            control,
            key_code: None,
        })
    }

    /// The `mod+mod+key` config string for this stroke.
    /// Swift `ShortcutStroke.configString(preserveDigit:)` (:2425-2433).
    pub fn config_string(&self, preserve_digit: bool) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.command {
            parts.push("cmd".to_string());
        }
        if self.shift {
            parts.push("shift".to_string());
        }
        if self.option {
            parts.push("opt".to_string());
        }
        if self.control {
            parts.push("ctrl".to_string());
        }
        parts.push(self.config_key_string(preserve_digit));
        parts.join("+")
    }

    /// Swift `ShortcutStroke.configKeyString(preserveDigit:)` (:2435-2442).
    fn config_key_string(&self, preserve_digit: bool) -> String {
        if let Ok(digit) = self.key.parse::<i32>() {
            if (1..=9).contains(&digit) {
                return if preserve_digit {
                    self.key.clone()
                } else {
                    "1".to_string()
                };
            }
        }
        if self.key == "\r" {
            return "return".to_string();
        }
        if self.key == "\t" {
            return "tab".to_string();
        }
        self.key.clone()
    }

    /// Internal ctor mirroring Swift `StoredShortcut(key:command:shift:option:control:)`'s
    /// per-stroke initializer (no explicit key code).
    fn simple(key: &str, command: bool, shift: bool, option: bool, control: bool) -> Self {
        Self {
            key: key.to_string(),
            command,
            shift,
            option,
            control,
            key_code: None,
        }
    }
}

impl StoredShortcut {
    /// Single-stroke convenience ctor mirroring Swift
    /// `StoredShortcut(key:command:shift:option:control:)`.
    fn single(key: &str, command: bool, shift: bool, option: bool, control: bool) -> Self {
        Self {
            first: ShortcutStroke::simple(key, command, shift, option, control),
            second: None,
        }
    }

    /// Two-stroke (chord) convenience ctor.
    fn chord(first: ShortcutStroke, second: ShortcutStroke) -> Self {
        Self {
            first,
            second: Some(second),
        }
    }

    /// Whether a config token means "no binding".
    /// Swift `StoredShortcut.isUnboundConfigToken` (:2546-2552).
    /// NOTE: a single `" "` is NOT unbound (it maps to the space key).
    pub fn is_unbound_config_token(raw_value: &str) -> bool {
        if raw_value.is_empty() {
            return true;
        }
        if raw_value == " " {
            return false;
        }
        let normalized = raw_value.trim().to_lowercase();
        if normalized.is_empty() {
            return true;
        }
        matches!(
            normalized.as_str(),
            "none" | "clear" | "unbound" | "disabled"
        )
    }

    /// Parse a config value that is a single stroke string (or unbound sentinel).
    /// Swift `StoredShortcut.parseConfig(_:allowBareFirstStroke:)` (:2517-2522).
    pub fn parse_config(raw_value: &str, allow_bare_first_stroke: bool) -> Option<StoredShortcut> {
        if Self::is_unbound_config_token(raw_value) {
            return Some(Self::unbound());
        }
        Self::parse_config_strokes(&[raw_value.to_string()], allow_bare_first_stroke)
    }

    /// Parse a config value from an explicit list of stroke strings (chords).
    /// Swift `StoredShortcut.parseConfig(strokes:allowBareFirstStroke:)` (:2524-2536).
    pub fn parse_config_strokes(
        strokes: &[String],
        allow_bare_first_stroke: bool,
    ) -> Option<StoredShortcut> {
        if strokes.is_empty() || strokes.len() > 2 {
            return None;
        }
        if strokes.len() == 1 {
            if let Some(raw_value) = strokes.first() {
                if Self::is_unbound_config_token(raw_value) {
                    return Some(Self::unbound());
                }
            }
        }
        let parsed_strokes: Vec<ShortcutStroke> = strokes
            .iter()
            .filter_map(|s| ShortcutStroke::parse_config(s))
            .collect();
        // Swift requires every stroke to parse (compactMap count == strokes count).
        if parsed_strokes.len() != strokes.len() {
            return None;
        }
        let first_stroke = parsed_strokes.first()?.clone();
        // Bare-first gate: allow || has-modifier || the space key.
        if !(allow_bare_first_stroke
            || first_stroke.has_any_modifier()
            || first_stroke.key == "space")
        {
            return None;
        }
        let second_stroke = if parsed_strokes.len() == 2 {
            Some(parsed_strokes[1].clone())
        } else {
            None
        };
        Some(StoredShortcut {
            first: first_stroke,
            second: second_stroke,
        })
    }

    /// The canonical config identifier (round-trips through `parseConfig`).
    /// Swift `StoredShortcut.configIdentifier` (:2538-2544).
    pub fn config_identifier(&self) -> String {
        if self.is_unbound() {
            return "none".to_string();
        }
        if let Some(second_stroke) = &self.second {
            return format!(
                "{} {}",
                self.first.config_string(true),
                second_stroke.config_string(true)
            );
        }
        self.first.config_string(true)
    }
}

// ---------------------------------------------------------------------------
// (2) DISPLAY FORMATTING
// Swift ref: ShortcutDisplayFormatter.swift:1-165.
//
// English `defaultValue` labels are used verbatim for oracle parity. The
// localization keys (`shortcut.*.label`, `shortcut.key.*`) are noted in
// comments for a later i18n lane; do NOT hardcode non-English strings here.
// ---------------------------------------------------------------------------

/// Formats modifier booleans in cmux's standard `Control, Option, Shift, Command`
/// glyph order. Swift `modifierDisplayString(command:shift:option:control:)`.
pub fn modifier_display_string(command: bool, shift: bool, option: bool, control: bool) -> String {
    let mut result = String::new();
    if control {
        result.push('⌃');
    }
    if option {
        result.push('⌥');
    }
    if shift {
        result.push('⇧');
    }
    if command {
        result.push('⌘');
    }
    result
}

/// Modifier glyphs for a stroke. Swift `modifierDisplayString(_ stroke:)`.
pub fn modifier_display_string_for_stroke(stroke: &ShortcutStroke) -> String {
    modifier_display_string(stroke.command, stroke.shift, stroke.option, stroke.control)
}

/// Whether a key token is a valid numbered-shortcut placeholder digit `1..9`.
/// Swift `isNumberedDigitKey`.
pub fn is_numbered_digit_key(key: &str) -> bool {
    matches!(key.parse::<i32>(), Ok(digit) if (1..=9).contains(&digit))
}

/// English label for `f1`..`f20`. Swift `functionKeyDisplayString(for:)`.
pub fn function_key_display_string(key: &str) -> Option<String> {
    let rest = key.strip_prefix('f')?;
    let number: u32 = rest.parse().ok()?;
    if (1..=20).contains(&number) {
        Some(format!("F{number}"))
    } else {
        None
    }
}

/// English display label for a stored key token. Swift `keyDisplayString(_:)`.
pub fn key_display_string(key: &str) -> String {
    match key {
        "\t" => "Tab".to_string(),      // i18n: shortcut.key.tab
        "space" => "Space".to_string(), // i18n: shortcut.key.space
        "\r" => "↩".to_string(),
        "media.brightnessDown" => "Brightness Down".to_string(), // i18n: shortcut.key.mediaBrightnessDown
        "media.brightnessUp" => "Brightness Up".to_string(), // i18n: shortcut.key.mediaBrightnessUp
        "media.mute" => "Mute".to_string(),                  // i18n: shortcut.key.mediaMute
        "media.next" => "Next Track".to_string(),            // i18n: shortcut.key.mediaNext
        "media.playPause" => "Play/Pause".to_string(),       // i18n: shortcut.key.mediaPlayPause
        "media.previous" => "Previous Track".to_string(),    // i18n: shortcut.key.mediaPrevious
        "media.volumeDown" => "Volume Down".to_string(),     // i18n: shortcut.key.mediaVolumeDown
        "media.volumeUp" => "Volume Up".to_string(),         // i18n: shortcut.key.mediaVolumeUp
        _ => {
            if let Some(function_key) = function_key_display_string(key) {
                function_key
            } else {
                key.to_uppercase()
            }
        }
    }
}

/// Full display string for a single stroke (modifiers + key label).
/// Swift `displayString(_ stroke:)` / `strokeDisplayString`.
pub fn stroke_display_string(stroke: &ShortcutStroke) -> String {
    modifier_display_string_for_stroke(stroke) + &key_display_string(&stroke.key)
}

/// The range label shown for numbered workspace/surface shortcut families.
/// Swift `numberedDigitRangeHint`.
pub const NUMBERED_DIGIT_RANGE_HINT: &str = "1…9";

/// Full display string for a stored shortcut, optionally collapsing a `1..9`
/// digit to the range hint. Swift `displayString(_ shortcut:numbered:)`.
pub fn display_string(shortcut: &StoredShortcut, numbered: bool) -> String {
    if shortcut.is_unbound() {
        return "None".to_string(); // i18n: shortcut.unbound.displayValue
    }
    if numbered {
        if let Some(second) = &shortcut.second {
            if is_numbered_digit_key(&second.key) {
                return stroke_display_string(&shortcut.first)
                    + " "
                    + &modifier_display_string_for_stroke(second)
                    + NUMBERED_DIGIT_RANGE_HINT;
            }
        } else if is_numbered_digit_key(&shortcut.first.key) {
            return modifier_display_string_for_stroke(&shortcut.first) + NUMBERED_DIGIT_RANGE_HINT;
        }
    }
    if let Some(second) = &shortcut.second {
        return format!(
            "{} {}",
            stroke_display_string(&shortcut.first),
            stroke_display_string(second)
        );
    }
    stroke_display_string(&shortcut.first)
}

/// Pure display formatter mirroring Swift `ShortcutDisplayFormatter`. Thin
/// wrapper over the free functions above for call-site parity.
#[derive(Debug, Clone, Copy, Default)]
pub struct ShortcutDisplayFormatter;

impl ShortcutDisplayFormatter {
    pub fn new() -> Self {
        Self
    }

    pub fn numbered_digit_range_hint(&self) -> &'static str {
        NUMBERED_DIGIT_RANGE_HINT
    }

    pub fn display_string(&self, shortcut: &StoredShortcut, numbered: bool) -> String {
        display_string(shortcut, numbered)
    }

    pub fn stroke_display_string(&self, stroke: &ShortcutStroke) -> String {
        stroke_display_string(stroke)
    }

    pub fn modifier_display_string(
        &self,
        command: bool,
        shift: bool,
        option: bool,
        control: bool,
    ) -> String {
        modifier_display_string(command, shift, option, control)
    }

    pub fn key_display_string(&self, key: &str) -> String {
        key_display_string(key)
    }

    pub fn is_numbered_digit_key(&self, key: &str) -> bool {
        is_numbered_digit_key(key)
    }
}

// ---------------------------------------------------------------------------
// when-clause construction helpers (private, for default_focus_when_clause).
// ---------------------------------------------------------------------------

fn wc_atom(atom: ShortcutFocusAtom) -> ShortcutWhenClause {
    ShortcutWhenClause::Atom(atom)
}

fn wc_key(name: &str) -> ShortcutWhenClause {
    ShortcutWhenClause::Key(name.to_string())
}

fn wc_not(clause: ShortcutWhenClause) -> ShortcutWhenClause {
    ShortcutWhenClause::Not(Box::new(clause))
}

fn wc_and(lhs: ShortcutWhenClause, rhs: ShortcutWhenClause) -> ShortcutWhenClause {
    ShortcutWhenClause::And(Box::new(lhs), Box::new(rhs))
}

// ---------------------------------------------------------------------------
// (3) Action METADATA
// Reconciles the two drifted Swift enums onto the single Rust `Action`:
//   - app-target `KeyboardShortcutSettings.Action`
//       (defaultShortcut :319-576; isPublicShortcutAction :306-317;
//        usesNumberedDigitMatching/allowsBareFirstStroke/allowsChordShortcut/
//        isBrowserContentShortcut :582-621)
//   - package `CmuxSettings.ShortcutAction`
//       (defaultFocusWhenClause/hasPriorityShortcutRouting :262-316;
//        flag parity :220-253)
// ---------------------------------------------------------------------------

impl Action {
    /// The factory default binding. Swift `Action.defaultShortcut` (:319-576).
    pub fn default_shortcut(&self) -> StoredShortcut {
        use Action::*;
        match self {
            OpenSettings => StoredShortcut::single(",", true, false, false, false),
            ReloadConfiguration => StoredShortcut::single(",", true, true, false, false),
            // Ctrl+Option+Cmd+. — avoids AppKit-reserved Cmd+. modal cancel.
            ShowHideAllWindows => StoredShortcut::single(".", true, false, true, true),
            GlobalSearch => StoredShortcut::single("f", true, false, true, false),
            NewWindow => StoredShortcut::single("n", true, true, false, false),
            CloseWindow => StoredShortcut::single("w", true, false, false, true),
            ToggleFullScreen => StoredShortcut::single("f", true, false, false, true),
            Quit => StoredShortcut::single("q", true, false, false, false),
            ToggleSidebar => StoredShortcut::single("b", true, false, false, false),
            NewTab => StoredShortcut::single("n", true, false, false, false),
            NewBrowserWorkspace => StoredShortcut::single("n", true, false, true, false),
            SaveLayoutTemplate => StoredShortcut::single("s", true, false, false, true),
            OpenFolder => StoredShortcut::single("o", true, false, false, false),
            ReopenPreviousSession => StoredShortcut::single("o", true, true, false, false),
            GoToWorkspace => StoredShortcut::single("p", true, false, false, false),
            CommandPalette => StoredShortcut::single("p", true, true, false, false),
            CommandPaletteNext => StoredShortcut::single("n", false, false, false, true),
            CommandPalettePrevious => StoredShortcut::single("p", false, false, false, true),
            SendFeedback => StoredShortcut::unbound(),
            ShowNotifications => StoredShortcut::single("i", true, false, false, false),
            JumpToUnread => StoredShortcut::single("u", true, true, false, false),
            ToggleUnread => StoredShortcut::single("u", true, false, true, false),
            MarkOldestUnreadAndJumpNext => StoredShortcut::single("u", true, false, false, true),
            FocusRightSidebar => StoredShortcut::single("e", true, true, false, false),
            SwitchRightSidebarToFiles => StoredShortcut::single("1", false, false, false, true),
            SwitchRightSidebarToFind => StoredShortcut::single("2", false, false, false, true),
            SwitchRightSidebarToSessions => StoredShortcut::single("3", false, false, false, true),
            SwitchRightSidebarToFeed => StoredShortcut::single("4", false, false, false, true),
            SwitchRightSidebarToDock => StoredShortcut::single("5", false, false, false, true),
            TriggerFlash => StoredShortcut::single("h", true, true, false, false),
            NextSidebarTab => StoredShortcut::single("]", true, false, false, true),
            PrevSidebarTab => StoredShortcut::single("[", true, false, false, true),
            FocusHistoryBack => StoredShortcut::single("[", true, false, false, false),
            FocusHistoryForward => StoredShortcut::single("]", true, false, false, false),
            RenameTab => StoredShortcut::single("r", true, false, false, false),
            RenameWorkspace => StoredShortcut::single("r", true, true, false, false),
            EditWorkspaceDescription => StoredShortcut::single("e", true, false, true, false),
            CloseTab => StoredShortcut::single("w", true, false, false, false),
            CloseOtherTabsInPane => StoredShortcut::single("t", true, false, true, false),
            CloseWorkspace => StoredShortcut::single("w", true, true, false, false),
            NewWorkspaceGroup => StoredShortcut::single("g", true, false, false, true),
            GroupSelectedWorkspaces => StoredShortcut::single("g", true, true, false, false),
            ToggleFocusedWorkspaceGroupCollapsed => {
                StoredShortcut::single(".", true, false, false, true)
            }
            ReopenClosedBrowserPanel => StoredShortcut::single("t", true, true, false, false),
            FocusLeft => StoredShortcut::single("←", true, false, true, false),
            FocusRight => StoredShortcut::single("→", true, false, true, false),
            FocusUp => StoredShortcut::single("↑", true, false, true, false),
            FocusDown => StoredShortcut::single("↓", true, false, true, false),
            SplitRight => StoredShortcut::single("d", true, false, false, false),
            SplitDown => StoredShortcut::single("d", true, true, false, false),
            ToggleSplitZoom => StoredShortcut::single("\r", true, true, false, false),
            EqualizeSplits => StoredShortcut::single("=", true, false, false, true),
            SplitBrowserRight => StoredShortcut::single("d", true, false, true, false),
            SplitBrowserDown => StoredShortcut::single("d", true, true, true, false),
            ToggleCanvasLayout => StoredShortcut::single("c", true, false, false, true),
            CanvasRevealFocusedPane => StoredShortcut::single("r", true, false, false, true),
            CanvasOverview => StoredShortcut::single("o", true, false, false, true),
            CanvasZoomIn => StoredShortcut::single("=", true, false, true, false),
            CanvasZoomOut => StoredShortcut::single("-", true, false, true, false),
            CanvasZoomReset => StoredShortcut::single("0", true, false, false, false),
            CanvasTidy => StoredShortcut::single("t", true, false, false, true),
            // Unbound by default: reachable via command palette / canvas.* socket verbs.
            CanvasAlignLeft
            | CanvasAlignRight
            | CanvasAlignTop
            | CanvasAlignBottom
            | CanvasEqualizeWidths
            | CanvasEqualizeHeights
            | CanvasDistributeHorizontally
            | CanvasDistributeVertically => StoredShortcut::unbound(),
            NextSurface => StoredShortcut::single("]", true, true, false, false),
            PrevSurface => StoredShortcut::single("[", true, true, false, false),
            SelectSurfaceByNumber => StoredShortcut::single("1", false, false, false, true),
            NewSurface => StoredShortcut::single("t", true, false, false, false),
            ToggleTerminalCopyMode => StoredShortcut::single("m", true, true, false, false),
            FocusTextBoxInput => StoredShortcut::single("a", true, true, false, false),
            CycleTextBoxSubmitAction => StoredShortcut::single("\t", false, true, false, false),
            AttachTextBoxFile => StoredShortcut::single("a", true, true, true, false),
            // Unbound by default: deliberate escape hatch, opt-in via Settings.
            SendCtrlFToTerminal => StoredShortcut::unbound(),
            ClearScreenKeepScrollback => StoredShortcut::single("k", true, true, false, false),
            SelectWorkspaceByNumber => StoredShortcut::single("1", true, false, false, false),
            ToggleRightSidebar => StoredShortcut::single("b", true, false, true, false),
            FileExplorerOpenSelection => StoredShortcut::single("\r", false, false, false, false),
            FileExplorerOpenSelectionFinderAlias => {
                StoredShortcut::single("↓", true, false, false, false)
            }
            SaveFilePreview => StoredShortcut::single("s", true, false, false, false),
            OpenBrowser => StoredShortcut::single("l", true, true, false, false),
            FocusBrowserAddressBar => StoredShortcut::single("l", true, false, false, false),
            BrowserBack => StoredShortcut::single("[", true, false, false, false),
            BrowserForward => StoredShortcut::single("]", true, false, false, false),
            BrowserReload => StoredShortcut::single("r", true, false, false, false),
            BrowserHardReload => StoredShortcut::single("r", true, true, false, false),
            BrowserZoomIn => StoredShortcut::single("=", true, false, false, false),
            BrowserZoomOut => StoredShortcut::single("-", true, false, false, false),
            BrowserZoomReset => StoredShortcut::single("0", true, false, false, false),
            MarkdownZoomIn => StoredShortcut::single("=", true, false, false, false),
            MarkdownZoomOut => StoredShortcut::single("-", true, false, false, false),
            MarkdownZoomReset => StoredShortcut::single("0", true, false, false, false),
            Find => StoredShortcut::single("f", true, false, false, false),
            FindInDirectory => StoredShortcut::single("f", true, true, false, false),
            FindNext => StoredShortcut::single("g", true, false, false, false),
            FindPrevious => StoredShortcut::single("g", true, false, true, false),
            HideFind => StoredShortcut::single("f", true, true, true, false),
            UseSelectionForFind => StoredShortcut::single("e", true, false, false, false),
            ToggleBrowserDeveloperTools => StoredShortcut::single("i", true, false, true, false),
            ShowBrowserJavaScriptConsole => StoredShortcut::single("c", true, false, true, false),
            ToggleBrowserFocusMode => StoredShortcut::single("\r", true, false, true, false),
            ToggleReactGrab => StoredShortcut::single("g", true, true, false, false),
            OpenDiffViewer => StoredShortcut::single("d", true, true, false, true),
            DiffViewerScrollDown => StoredShortcut::single("j", false, false, false, false),
            DiffViewerScrollUp => StoredShortcut::single("k", false, false, false, false),
            DiffViewerScrollToBottom => StoredShortcut::single("g", false, true, false, false),
            // The `g g` chord default.
            DiffViewerScrollToTop => StoredShortcut::chord(
                ShortcutStroke::simple("g", false, false, false, false),
                ShortcutStroke::simple("g", false, false, false, false),
            ),
            DiffViewerOpenFileSearch => StoredShortcut::single("/", false, false, false, false),
        }
    }

    /// Whether this action binds the whole `1..9` digit range via one placeholder.
    /// Swift `usesNumberedDigitMatching` (:582-589 / ShortcutAction :220-227).
    pub fn uses_numbered_digit_matching(&self) -> bool {
        matches!(
            self,
            Action::SelectSurfaceByNumber | Action::SelectWorkspaceByNumber
        )
    }

    /// Whether the recorder may accept a first stroke with no modifier.
    /// Swift `allowsBareFirstStroke` (:591-604 / ShortcutAction :235-248).
    pub fn allows_bare_first_stroke(&self) -> bool {
        matches!(
            self,
            Action::DiffViewerScrollDown
                | Action::DiffViewerScrollUp
                | Action::DiffViewerScrollToBottom
                | Action::DiffViewerScrollToTop
                | Action::DiffViewerOpenFileSearch
                | Action::FileExplorerOpenSelection
                | Action::FileExplorerOpenSelectionFinderAlias
        )
    }

    /// Whether this action supports a two-stroke chord.
    /// Swift `allowsChordShortcut` (:606-608 / ShortcutAction :251-253).
    pub fn allows_chord_shortcut(&self) -> bool {
        !matches!(
            self,
            Action::FileExplorerOpenSelection
                | Action::FileExplorerOpenSelectionFinderAlias
                | Action::CycleTextBoxSubmitAction
        )
    }

    /// Whether this shortcut is a browser-content (diff-viewer) navigation key.
    /// Swift `isBrowserContentShortcut` (:610-621).
    pub fn is_browser_content_shortcut(&self) -> bool {
        matches!(
            self,
            Action::DiffViewerScrollDown
                | Action::DiffViewerScrollUp
                | Action::DiffViewerScrollToBottom
                | Action::DiffViewerScrollToTop
                | Action::DiffViewerOpenFileSearch
        )
    }

    /// Whether this action is surfaced in the public shortcut list.
    /// Swift `isPublicShortcutAction` (:306-317).
    pub fn is_public_shortcut_action(&self) -> bool {
        !matches!(
            self,
            Action::SwitchRightSidebarToFiles
                | Action::SwitchRightSidebarToFind
                | Action::SwitchRightSidebarToSessions
                | Action::SwitchRightSidebarToFeed
                | Action::SwitchRightSidebarToDock
        )
    }

    /// Whether the key router consumes this action *before* general matching
    /// while its context holds. Swift `hasPriorityShortcutRouting` (:308-316).
    pub fn has_priority_shortcut_routing(&self) -> bool {
        matches!(
            self,
            Action::SwitchRightSidebarToFiles
                | Action::SwitchRightSidebarToFind
                | Action::SwitchRightSidebarToSessions
                | Action::SwitchRightSidebarToFeed
                | Action::SwitchRightSidebarToDock
        )
    }

    /// The action's built-in focus context as a `ShortcutWhenClause`, used when
    /// no `shortcuts.when` override applies. Swift `defaultFocusWhenClause`
    /// (ShortcutAction.swift :262-295).
    pub fn default_focus_when_clause(&self) -> ShortcutWhenClause {
        use Action::*;
        match self {
            SwitchRightSidebarToFiles
            | SwitchRightSidebarToFind
            | SwitchRightSidebarToSessions
            | SwitchRightSidebarToFeed
            | SwitchRightSidebarToDock
            | FileExplorerOpenSelection
            | FileExplorerOpenSelectionFinderAlias => wc_atom(ShortcutFocusAtom::SidebarFocus),
            RenameTab | RenameWorkspace | SendCtrlFToTerminal | ClearScreenKeepScrollback => {
                wc_and(
                    wc_not(wc_atom(ShortcutFocusAtom::BrowserFocus)),
                    wc_not(wc_atom(ShortcutFocusAtom::SidebarFocus)),
                )
            }
            BrowserBack
            | BrowserForward
            | BrowserReload
            | BrowserHardReload
            | ToggleBrowserDeveloperTools
            | ShowBrowserJavaScriptConsole
            | BrowserZoomIn
            | BrowserZoomOut
            | BrowserZoomReset
            | ToggleBrowserFocusMode
            | DiffViewerScrollDown
            | DiffViewerScrollUp
            | DiffViewerScrollToBottom
            | DiffViewerScrollToTop
            | DiffViewerOpenFileSearch => wc_atom(ShortcutFocusAtom::BrowserFocus),
            MarkdownZoomIn | MarkdownZoomOut | MarkdownZoomReset => {
                wc_atom(ShortcutFocusAtom::MarkdownFocus)
            }
            CanvasZoomReset => wc_and(
                wc_key("workspaceCanvasLayout"),
                wc_and(
                    wc_not(wc_atom(ShortcutFocusAtom::BrowserFocus)),
                    wc_not(wc_atom(ShortcutFocusAtom::MarkdownFocus)),
                ),
            ),
            CanvasRevealFocusedPane
            | CanvasOverview
            | CanvasZoomIn
            | CanvasZoomOut
            | CanvasTidy
            | CanvasAlignLeft
            | CanvasAlignRight
            | CanvasAlignTop
            | CanvasAlignBottom
            | CanvasEqualizeWidths
            | CanvasEqualizeHeights
            | CanvasDistributeHorizontally
            | CanvasDistributeVertically => wc_key("workspaceCanvasLayout"),
            _ => ShortcutWhenClause::Always,
        }
    }
}

// ---------------------------------------------------------------------------
// (4) CONFLICT DETECTION + RECORDER NORMALIZATION
// Swift refs: KeyboardShortcutSettings.swift :633-745, :841-960, :51-62.
// ---------------------------------------------------------------------------

/// Two concrete strokes collide iff key + all four modifiers are identical.
/// Swift `strokesConflict` (:954-960).
pub fn strokes_conflict(lhs: &ShortcutStroke, rhs: &ShortcutStroke) -> bool {
    lhs.key == rhs.key
        && lhs.command == rhs.command
        && lhs.shift == rhs.shift
        && lhs.option == rhs.option
        && lhs.control == rhs.control
}

/// Whether a stroke's key is a `1..9` digit. Swift `isNumberedDigitStroke` (:949-952).
pub fn is_numbered_digit_stroke(stroke: &ShortcutStroke) -> bool {
    matches!(stroke.key.parse::<i32>(), Ok(digit) if (1..=9).contains(&digit))
}

/// Two numbered-digit strokes collide iff both are digits with equal modifiers
/// (any digit stands in for the whole family). Swift `numberedDigitStrokeConflict` (:941-947).
pub fn numbered_digit_stroke_conflict(lhs: &ShortcutStroke, rhs: &ShortcutStroke) -> bool {
    if !is_numbered_digit_stroke(lhs) || !is_numbered_digit_stroke(rhs) {
        return false;
    }
    lhs.command == rhs.command
        && lhs.shift == rhs.shift
        && lhs.option == rhs.option
        && lhs.control == rhs.control
}

/// A numbered-digit family stroke vs an exact stroke collide iff both are
/// digits with equal modifiers. Swift `numberedDigitStrokeConflictsWithExactStroke` (:928-939).
pub fn numbered_digit_stroke_conflicts_with_exact_stroke(
    numbered_stroke: &ShortcutStroke,
    exact_stroke: &ShortcutStroke,
) -> bool {
    if !is_numbered_digit_stroke(numbered_stroke) || !is_numbered_digit_stroke(exact_stroke) {
        return false;
    }
    numbered_stroke.command == exact_stroke.command
        && numbered_stroke.shift == exact_stroke.shift
        && numbered_stroke.option == exact_stroke.option
        && numbered_stroke.control == exact_stroke.control
}

/// How a stroke matches: as an exact keystroke or as the `1..9` digit family.
/// Swift `ShortcutConflictMatchMode` (:858-861).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShortcutConflictMatchMode {
    Exact,
    NumberedDigitFamily,
}

/// Resolve a stroke-vs-stroke conflict under each side's match mode.
/// Swift `shortcutStrokeMatchersConflict` (:910-926).
pub fn shortcut_stroke_matchers_conflict(
    lhs: &ShortcutStroke,
    lhs_mode: ShortcutConflictMatchMode,
    rhs: &ShortcutStroke,
    rhs_mode: ShortcutConflictMatchMode,
) -> bool {
    use ShortcutConflictMatchMode::*;
    match (lhs_mode, rhs_mode) {
        (Exact, Exact) => strokes_conflict(lhs, rhs),
        (NumberedDigitFamily, NumberedDigitFamily) => numbered_digit_stroke_conflict(lhs, rhs),
        (NumberedDigitFamily, Exact) => numbered_digit_stroke_conflicts_with_exact_stroke(lhs, rhs),
        (Exact, NumberedDigitFamily) => numbered_digit_stroke_conflicts_with_exact_stroke(rhs, lhs),
    }
}

/// The 4-arm chord matrix that decides whether two bindings share a keystroke.
/// Swift `shortcutsConflict` (:863-908).
pub fn shortcuts_conflict(
    proposed_shortcut: &StoredShortcut,
    proposed_uses_numbered_digit_matching: bool,
    configured_shortcut: &StoredShortcut,
    configured_uses_numbered_digit_matching: bool,
) -> bool {
    use ShortcutConflictMatchMode::*;
    if proposed_shortcut.is_unbound() || configured_shortcut.is_unbound() {
        return false;
    }

    let proposed_mode = |flag: bool| if flag { NumberedDigitFamily } else { Exact };
    let proposed_first_mode = proposed_mode(proposed_uses_numbered_digit_matching);
    let configured_first_mode = proposed_mode(configured_uses_numbered_digit_matching);

    match (
        proposed_shortcut.has_chord(),
        configured_shortcut.has_chord(),
    ) {
        (false, false) => shortcut_stroke_matchers_conflict(
            &proposed_shortcut.first,
            proposed_first_mode,
            &configured_shortcut.first,
            configured_first_mode,
        ),
        (true, true) => {
            if !strokes_conflict(&proposed_shortcut.first, &configured_shortcut.first) {
                return false;
            }
            let (Some(proposed_second), Some(configured_second)) =
                (&proposed_shortcut.second, &configured_shortcut.second)
            else {
                return false;
            };
            shortcut_stroke_matchers_conflict(
                proposed_second,
                proposed_first_mode,
                configured_second,
                configured_first_mode,
            )
        }
        (true, false) => shortcut_stroke_matchers_conflict(
            &proposed_shortcut.first,
            Exact,
            &configured_shortcut.first,
            configured_first_mode,
        ),
        (false, true) => shortcut_stroke_matchers_conflict(
            &proposed_shortcut.first,
            proposed_first_mode,
            &configured_shortcut.first,
            Exact,
        ),
    }
}

/// Recorder rejection reasons. Swift `ShortcutRecordingRejection` (:51-57).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutRecordingRejection {
    BareKeyNotAllowed,
    ConflictsWithAction(Action),
    ReservedBySystem,
    NumberedShortcutRequiresDigit,
    SystemWideHotkeyRequiresModifier,
}

/// Result of resolving a recorded shortcut. Swift `RecordedShortcutResolution` (:59-62).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedShortcutResolution {
    Accepted(StoredShortcut),
    Rejected(ShortcutRecordingRejection),
}

impl Action {
    /// Whether `self`'s configured binding collides with `proposed_shortcut`
    /// (owned by `proposed_action`). Swift `Action.conflicts` (:633-658).
    ///
    /// DIVERGENCE: Swift consults `effectiveWhenClause` = settings-file override
    /// OR the built-in default context. This headless port has no global
    /// settings store, so it uses `default_focus_when_clause` directly; a later
    /// lane can layer the `shortcuts.when` override on top.
    pub fn conflicts(
        &self,
        proposed_shortcut: &StoredShortcut,
        proposed_action: Action,
        configured_shortcut: &StoredShortcut,
    ) -> bool {
        if !ShortcutWhenClause::bindings_collide(
            &self.default_focus_when_clause(),
            self.has_priority_shortcut_routing(),
            &proposed_action.default_focus_when_clause(),
            proposed_action.has_priority_shortcut_routing(),
        ) {
            return false;
        }
        shortcuts_conflict(
            proposed_shortcut,
            proposed_action.uses_numbered_digit_matching(),
            configured_shortcut,
            self.uses_numbered_digit_matching(),
        )
    }

    /// Normalize a numbered-digit shortcut, folding any `1..9` digit to `"1"`.
    /// Swift `resolvedNumberedDigitShortcut` (:723-737).
    pub fn resolved_numbered_digit_shortcut(
        &self,
        shortcut: &StoredShortcut,
    ) -> RecordedShortcutResolution {
        let digit_source = shortcut.second.as_ref().unwrap_or(&shortcut.first);
        let is_digit = matches!(digit_source.key.parse::<i32>(), Ok(d) if (1..=9).contains(&d));
        if !is_digit {
            return RecordedShortcutResolution::Rejected(
                ShortcutRecordingRejection::NumberedShortcutRequiresDigit,
            );
        }
        let mut normalized = shortcut.clone();
        if let Some(second) = normalized.second.as_mut() {
            second.key = "1".to_string();
        } else {
            normalized.first.key = "1".to_string();
        }
        RecordedShortcutResolution::Accepted(normalized)
    }

    /// Resolve a recorded shortcut, applying action-specific normalization but
    /// NOT cross-action conflict checks. Swift
    /// `resolvedRecordedShortcutIgnoringConflicts` (:704-721).
    pub fn resolved_recorded_shortcut_ignoring_conflicts(
        &self,
        shortcut: &StoredShortcut,
    ) -> RecordedShortcutResolution {
        if shortcut.is_unbound() {
            return RecordedShortcutResolution::Accepted(StoredShortcut::unbound());
        }
        match self {
            Action::ShowHideAllWindows | Action::GlobalSearch => {
                // DIVERGENCE: the Carbon hotkey registration + system-wide
                // reserved-hotkey scan (`carbonHotKeyRegistration`,
                // `systemWideHotkeyConflicts`) are macOS-only and deferred to a
                // later Tauri lane. We port only the pure gates: chord rejection
                // and the require-modifier check via `has_primary_modifier`.
                // A shape the Carbon path would later reject is Accepted here.
                if shortcut.has_chord() {
                    return RecordedShortcutResolution::Rejected(
                        ShortcutRecordingRejection::ReservedBySystem,
                    );
                }
                if !shortcut.first.has_primary_modifier() {
                    return RecordedShortcutResolution::Rejected(
                        ShortcutRecordingRejection::SystemWideHotkeyRequiresModifier,
                    );
                }
                RecordedShortcutResolution::Accepted(shortcut.clone())
            }
            Action::SelectSurfaceByNumber | Action::SelectWorkspaceByNumber => {
                self.resolved_numbered_digit_shortcut(shortcut)
            }
            _ => RecordedShortcutResolution::Accepted(shortcut.clone()),
        }
    }

    /// Full recorder pipeline: chord gate, cross-action conflict, then
    /// action normalization. Swift `normalizedRecordedShortcutResult` (:660-676).
    pub fn normalized_recorded_shortcut_result(
        &self,
        shortcut: &StoredShortcut,
    ) -> RecordedShortcutResolution {
        if shortcut.is_unbound() {
            return RecordedShortcutResolution::Accepted(StoredShortcut::unbound());
        }
        if shortcut.has_chord() && !self.allows_chord_shortcut() {
            return RecordedShortcutResolution::Rejected(
                ShortcutRecordingRejection::ReservedBySystem,
            );
        }
        if let Some(conflicting_action) = conflicting_action(shortcut, *self) {
            return RecordedShortcutResolution::Rejected(
                ShortcutRecordingRejection::ConflictsWithAction(conflicting_action),
            );
        }
        self.resolved_recorded_shortcut_ignoring_conflicts(shortcut)
    }

    /// Normalize a shortcut loaded from cmux.json, without any global-store
    /// conflict/hotkey lookups. Swift `normalizedSettingsFileShortcut` (:678-702).
    pub fn normalized_settings_file_shortcut(
        &self,
        shortcut: &StoredShortcut,
    ) -> Option<StoredShortcut> {
        if shortcut.is_unbound() {
            return Some(StoredShortcut::unbound());
        }
        if shortcut.has_chord() && !self.allows_chord_shortcut() {
            return None;
        }
        if let RecordedShortcutResolution::Accepted(normalized) =
            self.resolved_recorded_shortcut_ignoring_conflicts(shortcut)
        {
            return Some(normalized);
        }
        // Preserve invalid settings-file values except for the numbered-digit
        // families and the global-search hotkey (Swift :698-701).
        if self.uses_numbered_digit_matching() || *self == Action::GlobalSearch {
            return None;
        }
        Some(shortcut.clone())
    }
}

/// The first action (other than `excluding`) whose factory-default binding
/// conflicts with `shortcut`. Swift `conflictingAction(for:excluding:)` (:841-856).
///
/// DIVERGENCE: Swift resolves each candidate's *configured* binding through the
/// global store (`shortcut(for:)`). This headless port has no store, so it
/// compares against each action's `default_shortcut()`; a later lane can inject
/// a configured-binding lookup.
pub fn conflicting_action(shortcut: &StoredShortcut, excluding: Action) -> Option<Action> {
    for action in Action::ALL {
        if action == excluding {
            continue;
        }
        let configured_shortcut = action.default_shortcut();
        if action.conflicts(shortcut, excluding, &configured_shortcut) {
            return Some(action);
        }
    }
    None
}

#[cfg(test)]
mod config_and_metadata_tests {
    use super::*;

    // (a) parse / format round-trip table.
    #[test]
    fn config_round_trips_are_idempotent_and_match_swift() {
        // (input, expected config_identifier, allow_bare)
        let cases: &[(&str, &str, bool)] = &[
            ("cmd+n", "cmd+n", false),
            // Input modifier order is free; config_identifier normalizes to the
            // canonical cmd, shift, opt, ctrl order (Swift configString).
            ("ctrl+opt+cmd+.", "cmd+opt+ctrl+.", false),
            ("cmd+shift+space", "cmd+shift+space", false),
            (" ", "space", false),
            ("return", "return", true),
            ("cmd+left", "cmd+←", false),
            ("cmd+opt+down", "cmd+opt+↓", false),
            ("f5", "f5", true),
            ("cmd+f13", "cmd+f13", false),
        ];
        for (input, expected_id, allow_bare) in cases {
            let parsed = StoredShortcut::parse_config(input, *allow_bare)
                .unwrap_or_else(|| panic!("parse {input}"));
            assert_eq!(
                parsed.config_identifier(),
                *expected_id,
                "config identifier for {input}"
            );
            // Idempotency: identifier re-parses to the same shortcut.
            let reparsed = StoredShortcut::parse_config(&parsed.config_identifier(), *allow_bare)
                .unwrap_or_else(|| panic!("reparse {input}"));
            assert_eq!(reparsed, parsed, "round-trip for {input}");
        }
    }

    #[test]
    fn unbound_sentinels_parse_to_unbound() {
        for token in ["", "none", "clear", "unbound", "disabled", "   ", "\t"] {
            assert_eq!(
                StoredShortcut::parse_config(token, false),
                Some(StoredShortcut::unbound()),
                "token {token:?} should be unbound"
            );
        }
        assert!(StoredShortcut::unbound().config_identifier() == "none");
        // A single space is NOT unbound.
        assert!(!StoredShortcut::is_unbound_config_token(" "));
        assert_eq!(
            StoredShortcut::parse_config(" ", false).map(|s| s.config_identifier()),
            Some("space".to_string())
        );
    }

    #[test]
    fn space_key_config_variants_normalize() {
        for raw in [
            "space",
            "cmd+space",
            "shift+space",
            "cmd+shift+space",
            "ctrl+space",
            "opt+space",
        ] {
            let parsed = StoredShortcut::parse_config(raw, false).expect("parse space variant");
            assert_eq!(parsed.first.key, "space");
            assert_eq!(parsed.config_identifier(), raw);
        }
        assert_eq!(
            StoredShortcut::parse_config("cmd+shift+Space", false).map(|s| s.config_identifier()),
            Some("cmd+shift+space".to_string())
        );
        assert_eq!(
            StoredShortcut::parse_config("cmd+shift+<space>", false).map(|s| s.config_identifier()),
            Some("cmd+shift+space".to_string())
        );
        assert_eq!(
            StoredShortcut::parse_config("cmd+shift+spacebar", false)
                .map(|s| s.config_identifier()),
            Some("cmd+shift+space".to_string())
        );
        assert_eq!(
            StoredShortcut::parse_config("cmd+shift+ ", false).map(|s| s.config_identifier()),
            Some("cmd+shift+space".to_string())
        );
        // Trailing whitespace-only token after a modifier is invalid.
        assert_eq!(StoredShortcut::parse_config("cmd+shift+   ", false), None);
    }

    #[test]
    fn bare_first_stroke_gate_matches_swift() {
        // Bare key rejected unless allowed / space / modifier present.
        assert_eq!(StoredShortcut::parse_config("j", false), None);
        assert!(StoredShortcut::parse_config("j", true).is_some());
        // Chord round-trip via explicit stroke list (allow bare).
        let chord = StoredShortcut::parse_config_strokes(&["g".to_string(), "g".to_string()], true)
            .expect("chord");
        assert!(chord.has_chord());
        assert_eq!(chord.config_identifier(), "g g");
        // >2 strokes rejected.
        assert_eq!(
            StoredShortcut::parse_config_strokes(
                &["g".to_string(), "g".to_string(), "g".to_string()],
                true
            ),
            None
        );
    }

    #[test]
    fn return_key_round_trips() {
        let shortcut = StoredShortcut::parse_config("return", true).expect("return");
        assert_eq!(shortcut.first.key, "\r");
        assert!(!shortcut.first.command);
        assert_eq!(shortcut.config_identifier(), "return");
        assert_eq!(StoredShortcut::parse_config("enter", true), Some(shortcut));
        // fileExplorerOpenSelection default is bare return.
        assert_eq!(
            Action::FileExplorerOpenSelection
                .default_shortcut()
                .config_identifier(),
            "return"
        );
    }

    // (b) default_shortcut spot-checks.
    #[test]
    fn default_shortcut_spot_checks() {
        assert_eq!(
            Action::OpenSettings.default_shortcut().config_identifier(),
            "cmd+,"
        );
        let top = Action::DiffViewerScrollToTop.default_shortcut();
        assert!(top.has_chord());
        assert_eq!(top.config_identifier(), "g g");
        assert!(Action::SendFeedback.default_shortcut().is_unbound());
        assert!(Action::CanvasAlignLeft.default_shortcut().is_unbound());
        assert!(Action::SendCtrlFToTerminal.default_shortcut().is_unbound());
        assert_eq!(
            Action::SaveLayoutTemplate
                .default_shortcut()
                .config_identifier(),
            "cmd+ctrl+s"
        );
        assert_eq!(
            Action::NewWorkspaceGroup
                .default_shortcut()
                .config_identifier(),
            "cmd+ctrl+g"
        );
        assert_eq!(
            Action::CycleTextBoxSubmitAction
                .default_shortcut()
                .config_identifier(),
            "shift+tab"
        );
        assert!(!Action::CycleTextBoxSubmitAction.allows_chord_shortcut());
        // A few more precise modifier combos.
        assert_eq!(
            Action::ShowHideAllWindows
                .default_shortcut()
                .config_identifier(),
            "cmd+opt+ctrl+."
        );
        assert_eq!(
            Action::OpenDiffViewer
                .default_shortcut()
                .config_identifier(),
            "cmd+shift+ctrl+d"
        );
    }

    // (c) display parity.
    #[test]
    fn display_parity() {
        assert_eq!(
            display_string(&Action::NewTab.default_shortcut(), false),
            "⌘N"
        );
        assert_eq!(
            display_string(&Action::NewSurface.default_shortcut(), false),
            "⌘T"
        );
        // Ctrl, Opt, Shift, Cmd glyph order.
        let all_mods = StoredShortcut::single("x", true, true, true, true);
        assert_eq!(display_string(&all_mods, false), "⌃⌥⇧⌘X");
        // Numbered digit range hint.
        assert_eq!(
            display_string(&Action::SelectSurfaceByNumber.default_shortcut(), true),
            "⌃1…9"
        );
        assert_eq!(
            display_string(&Action::SelectWorkspaceByNumber.default_shortcut(), true),
            "⌘1…9"
        );
        // Named keys.
        assert_eq!(
            display_string(&Action::ToggleSplitZoom.default_shortcut(), false),
            "⇧⌘↩"
        );
        assert_eq!(
            display_string(
                &StoredShortcut::single("space", false, false, false, false),
                false
            ),
            "Space"
        );
        assert_eq!(
            display_string(
                &StoredShortcut::single("f7", false, false, false, false),
                true
            ),
            "F7"
        );
        assert_eq!(display_string(&StoredShortcut::unbound(), false), "None");
    }

    // (d) conflict parity.
    #[test]
    fn exact_and_numbered_conflict_parity() {
        let cmd_n = StoredShortcut::single("n", true, false, false, false);
        let cmd_m = StoredShortcut::single("m", true, false, false, false);
        assert!(shortcuts_conflict(&cmd_n, false, &cmd_n, false));
        assert!(!shortcuts_conflict(&cmd_n, false, &cmd_m, false));

        // Numbered-digit family: Ctrl+5 (exact) vs Ctrl+1 (numbered) collide.
        let ctrl_5 = StoredShortcut::single("5", false, false, false, true);
        let ctrl_1 = StoredShortcut::single("1", false, false, false, true);
        assert!(shortcuts_conflict(&ctrl_5, false, &ctrl_1, true));
        // Different modifier family does not.
        let cmd_1 = StoredShortcut::single("1", true, false, false, false);
        assert!(!shortcuts_conflict(&ctrl_5, false, &cmd_1, true));
    }

    #[test]
    fn action_level_conflict_uses_when_and_priority() {
        // Sidebar-priority coexistence: ⌃1 sidebar-files (priority) vs ⌃1..9
        // select-surface (always) must NOT conflict.
        let ctrl_1 = StoredShortcut::single("1", false, false, false, true);
        assert!(!Action::SwitchRightSidebarToFiles.conflicts(
            &ctrl_1,
            Action::SelectSurfaceByNumber,
            &ctrl_1,
        ));
        // A genuine overlap with no priority resolution DOES conflict:
        // focusHistoryBack (Cmd+[, always) vs browserBack (Cmd+[, browserFocus).
        let back = Action::BrowserBack.default_shortcut();
        assert!(Action::FocusHistoryBack.conflicts(&back, Action::BrowserBack, &back));
        // conflicting_action finds a colliding default for a fresh binding.
        assert!(
            conflicting_action(&Action::NewTab.default_shortcut(), Action::NewSurface).is_some()
        );
        // An unbound proposal never conflicts.
        assert_eq!(
            conflicting_action(&StoredShortcut::unbound(), Action::NewTab),
            None
        );
    }

    #[test]
    fn recorder_pipeline_parity() {
        // Numbered digit normalization folds the digit to "1".
        let ctrl_7 = StoredShortcut::single("7", false, false, false, true);
        assert_eq!(
            Action::SelectSurfaceByNumber.resolved_recorded_shortcut_ignoring_conflicts(&ctrl_7),
            RecordedShortcutResolution::Accepted(StoredShortcut::single(
                "1", false, false, false, true
            ))
        );
        // Numbered family requires a digit.
        let ctrl_a = StoredShortcut::single("a", false, false, false, true);
        assert_eq!(
            Action::SelectSurfaceByNumber.resolved_recorded_shortcut_ignoring_conflicts(&ctrl_a),
            RecordedShortcutResolution::Rejected(
                ShortcutRecordingRejection::NumberedShortcutRequiresDigit
            )
        );
        // Chord rejected for file-explorer open (no chord support).
        let chord = StoredShortcut::chord(
            ShortcutStroke::simple("g", false, false, false, false),
            ShortcutStroke::simple("g", false, false, false, false),
        );
        assert_eq!(
            Action::FileExplorerOpenSelection.normalized_recorded_shortcut_result(&chord),
            RecordedShortcutResolution::Rejected(ShortcutRecordingRejection::ReservedBySystem)
        );
        // Global hotkey without a primary modifier is rejected.
        let bare_shift = StoredShortcut::single("h", false, true, false, false);
        assert_eq!(
            Action::ShowHideAllWindows.resolved_recorded_shortcut_ignoring_conflicts(&bare_shift),
            RecordedShortcutResolution::Rejected(
                ShortcutRecordingRejection::SystemWideHotkeyRequiresModifier
            )
        );
        // Settings-file normalization keeps unbound and preserves valid values.
        assert_eq!(
            Action::NewTab.normalized_settings_file_shortcut(&StoredShortcut::unbound()),
            Some(StoredShortcut::unbound())
        );
    }

    // (e) serde wire-shape guard.
    #[test]
    fn serde_wire_shape_is_stable() {
        let shortcut = StoredShortcut {
            first: ShortcutStroke {
                key: "space".to_string(),
                command: true,
                shift: true,
                option: false,
                control: false,
                key_code: Some(0x31),
            },
            second: None,
        };
        let json = serde_json::to_value(&shortcut).expect("serialize");
        // {first, second?} + per-stroke camelCase keyCode.
        assert_eq!(json["first"]["key"], "space");
        assert_eq!(json["first"]["command"], true);
        assert_eq!(json["first"]["keyCode"], 0x31);
        assert!(json.get("second").is_none());

        // Parse from the canonical cmux.json wire shape.
        let parsed: StoredShortcut = serde_json::from_value(serde_json::json!({
            "first": { "key": "space", "command": true, "shift": true }
        }))
        .expect("deserialize");
        assert_eq!(parsed.first.key, "space");
        assert!(parsed.first.command && parsed.first.shift);
        assert!(parsed.first.key_code.is_none());

        // toggleSplitZoom bound to cmd+shift+space (oracle from
        // KeyboardShortcutSpaceKeyTests.settingsFileStoreParsesSpaceShortcutBinding).
        assert_eq!(
            StoredShortcut::parse_config("cmd+shift+space", false),
            Some(StoredShortcut::single("space", true, true, false, false))
        );
    }
}
