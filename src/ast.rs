//! Semantic AST (spec §2.3), stored in an arena and resolved in place.

use crate::diag::Span;
use crate::value::Value;
use std::rc::Rc;

pub type NodeId = u32;

/// A statically resolved variable reference (closure-converted).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Var {
    Unresolved,
    /// Slot in the current activation's locals.
    Local(u32),
    /// Index in the current closure's captured values.
    Capture(u32),
    /// The currently executing named function itself.
    SelfFn,
    /// A prelude binding (index into the prelude table).
    Prelude(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Not,
    Neg,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    Coalesce,
}

impl BinOp {
    pub fn symbol(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::Coalesce => "??",
        }
    }
}

/// Body of a named function (an expression) or lambda (a sequence).
#[derive(Clone, Debug)]
pub enum Body {
    Expr(NodeId),
    Seq(Vec<NodeId>),
}

#[derive(Clone, Debug)]
pub struct FnInfo {
    pub name: Option<Rc<str>>,
    pub params: Vec<Rc<str>>,
    pub slot_name: Option<Rc<str>>,
    pub body: Body,
    /// Number of local slots needed by one activation.
    pub nlocals: u32,
    /// How to obtain each captured value from the creating environment.
    pub captures: Vec<Var>,
    /// For a definition: the enclosing local slot receiving the function.
    pub def_slot: u32,
}

#[derive(Clone, Debug)]
pub enum Node {
    Lit(Value),
    /// A numeric literal whose exponent exceeds host range.
    NumOverflow,
    Array(Vec<NodeId>),
    Record(Vec<(Rc<str>, NodeId)>),
    Name { name: Rc<str>, var: Var },
    Lambda(FnInfo),
    Call { callee: NodeId, args: Vec<NodeId>, cb: Option<NodeId> },
    Pipe { left: NodeId, head: NodeId, fields: Vec<Rc<str>>, args: Vec<NodeId>, cb: Option<NodeId> },
    Get { recv: NodeId, key: NodeId, optional: bool },
    Unary { op: UnOp, e: NodeId },
    Binary { op: BinOp, l: NodeId, r: NodeId },
    If { c: NodeId, t: NodeId, e: NodeId },
    Do { items: Vec<NodeId> },
    Try { e: NodeId, name: Rc<str>, slot: u32, items: Vec<NodeId> },
    Query { segments: Rc<Vec<String>>, holes: Vec<NodeId> },
    Group(NodeId),
    Bind { name: Rc<str>, slot: u32, e: NodeId },
    Define(FnInfo),
}

#[derive(Clone, Debug, Default)]
pub struct Program {
    pub nodes: Vec<Node>,
    pub spans: Vec<Span>,
    pub items: Vec<NodeId>,
    pub nlocals: u32,
}

impl Program {
    pub fn add(&mut self, n: Node, span: Span) -> NodeId {
        self.nodes.push(n);
        self.spans.push(span);
        (self.nodes.len() - 1) as NodeId
    }

    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn span(&self, id: NodeId) -> Span {
        self.spans[id as usize]
    }

    /// Strip grouping nodes.
    pub fn strip_groups(&self, mut id: NodeId) -> NodeId {
        while let Node::Group(inner) = self.node(id) {
            id = *inner;
        }
        id
    }

    pub fn fn_info(&self, id: NodeId) -> &FnInfo {
        match self.node(id) {
            Node::Lambda(f) | Node::Define(f) => f,
            _ => panic!("node {id} is not a function"),
        }
    }
}
