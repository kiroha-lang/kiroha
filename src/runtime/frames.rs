//! Continuation frames.

use super::{Env, SeqOwner};
use crate::ast::NodeId;
use crate::value::{Record, Value};
use std::rc::Rc;

pub enum Frame {
    SeqBind { owner: SeqOwner, idx: usize, slot: u32, env: Rc<Env> },
    SeqExpr { owner: SeqOwner, idx: usize, env: Rc<Env> },
    Array { node: NodeId, acc: Vec<Value>, env: Rc<Env> },
    Record { node: NodeId, acc: Record, env: Rc<Env> },
    CallCallee { node: NodeId, env: Rc<Env> },
    CallArgs { node: NodeId, f: Value, args: Vec<Value>, env: Rc<Env> },
    PipeLeft { node: NodeId, env: Rc<Env> },
    PipeHead { node: NodeId, lhs: Value, env: Rc<Env> },
    PipeArgs { node: NodeId, f: Value, args: Vec<Value>, env: Rc<Env> },
    GetRecv { node: NodeId, env: Rc<Env> },
    GetKey { node: NodeId, recv: Value },
    Unary { node: NodeId },
    BinLeft { node: NodeId, env: Rc<Env> },
    BinRight { node: NodeId, left: Value },
    /// Right operand of `&&`/`||` must be bool.
    BoolRight { node: NodeId },
    If { node: NodeId, env: Rc<Env> },
    Try { node: NodeId, env: Rc<Env> },
    Query { node: NodeId, vals: Vec<Value>, env: Rc<Env> },
    /// Builtin continuation frames.
    Collect(Box<crate::builtins::callbacks::Collect>),
    Retry(Box<crate::builtins::callbacks::RetryFrame>),
    Pages(Box<crate::builtins::callbacks::PagesFrame>),
    Op(Box<crate::builtins::effects::OpFrame>),
    Tx(Box<crate::builtins::db::TxFrame>),
    Sql(Box<crate::builtins::db::SqlFrame>),
    Serve(u32),
    Request(Box<crate::builtins::server::RequestFrame>),
}
