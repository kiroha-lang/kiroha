//! Public signatures use quantified variables, JSON, or concrete structures.
//! They never introduce `any`: dynamic typing comes only from explicit source.
use super::Checker;
use crate::builtins::Builtin;
use crate::diag::{Diagnostic, Span};
use crate::types::{join, ty, Field, Function, Ty, Type};
use std::rc::Rc;

pub(super) fn array(t: Ty) -> Ty {
    ty(Type::Array(t))
}
pub(super) fn record(fields: Vec<(&str, Ty, bool)>) -> Ty {
    ty(Type::Record {
        fields: fields
            .into_iter()
            .map(|(name, ty, optional)| Field {
                name: Rc::from(name),
                ty,
                optional,
            })
            .collect(),
        rest: None,
    })
}
pub(super) fn dict(value: Ty) -> Ty {
    ty(Type::Record {
        fields: Vec::new(),
        rest: Some(value),
    })
}
pub(super) fn function(params: Vec<Ty>, result: Ty) -> Ty {
    let min = params.len();
    ty(Type::Function(Function {
        params,
        min,
        callback: None,
        optional_callback: false,
        result,
        generics: Vec::new(),
        builtin: None,
        receiver: None,
    }))
}
pub(super) fn error() -> Ty {
    record(vec![
        ("code", ty(Type::Str), false),
        ("message", ty(Type::Str), false),
        ("data", dict(ty(Type::Json)), false),
    ])
}
pub(super) fn request() -> Ty {
    record(vec![
        ("method", ty(Type::Str), false),
        ("path", ty(Type::Str), false),
        ("params", dict(ty(Type::Str)), false),
        ("query", dict(array(ty(Type::Str))), false),
        ("headers", dict(array(ty(Type::Str))), false),
        ("body", ty(Type::Bytes), false),
    ])
}
fn scalar() -> Ty {
    [Type::Str, Type::Num, Type::Bool, Type::Null]
        .into_iter()
        .map(ty)
        .fold(ty(Type::Never), join)
}
fn http_options(body: bool) -> Ty {
    let mut fields = vec![
        ("headers", dict(ty(Type::Str)), true),
        ("query", dict(join(scalar(), array(scalar()))), true),
        ("timeout", ty(Type::Int), true),
        ("max_bytes", ty(Type::Int), true),
    ];
    if body {
        fields.push(("body", ty(Type::Bytes), true));
    }
    record(fields)
}
impl Checker<'_> {
    pub(super) fn builtin_type(
        &mut self,
        b: Builtin,
        receiver: Option<Ty>,
        span: Span,
    ) -> Result<Ty, Diagnostic> {
        use Builtin::*;
        let mut generics = Vec::new();
        let mut generic = |label: &str| {
            let id = self.next_param;
            self.next_param += 1;
            generics.push(id);
            ty(Type::Param {
                id,
                name: Rc::from(label),
            })
        };
        let elem = if receiver.is_some()
            && matches!(
                b,
                Map | Filter
                    | Fold
                    | Each
                    | Flat
                    | Take
                    | Sort
                    | Group
                    | Unique
                    | Sum
                    | One
                    | Join
                    | Parallel
            ) {
            Some(generic("Item"))
        } else {
            None
        };
        let r = generic("Result");
        // Remove unused result parameters below by collecting free parameters.
        let s = ty(Type::Str);
        let n = ty(Type::Num);
        let i = ty(Type::Int);
        let j = ty(Type::Json);
        let null = ty(Type::Null);
        let e = elem.unwrap_or_else(|| ty(Type::Never));
        let mut expected_receiver = None;
        let (params, result, callback) = match b {
            Len => (vec![], i.clone(), None),
            Range => (
                vec![i.clone(), i.clone(), i.clone()],
                array(i.clone()),
                None,
            ),
            Map => {
                expected_receiver = Some(array(e.clone()));
                (
                    vec![],
                    array(r.clone()),
                    Some(function(vec![e.clone()], r.clone())),
                )
            }
            Filter => {
                expected_receiver = Some(array(e.clone()));
                (
                    vec![],
                    array(e.clone()),
                    Some(function(vec![e.clone()], ty(Type::Bool))),
                )
            }
            Fold => {
                expected_receiver = Some(array(e.clone()));
                (
                    vec![r.clone()],
                    r.clone(),
                    Some(function(vec![r.clone(), e.clone()], r.clone())),
                )
            }
            Each => {
                expected_receiver = Some(array(e.clone()));
                (
                    vec![],
                    null.clone(),
                    Some(function(vec![e.clone()], r.clone())),
                )
            }
            Flat => {
                expected_receiver = Some(array(array(e.clone())));
                (vec![], array(e.clone()), None)
            }
            Take => {
                expected_receiver = Some(array(e.clone()));
                (vec![i.clone()], array(e.clone()), None)
            }
            Sort => {
                expected_receiver = Some(array(e.clone()));
                (
                    vec![],
                    array(e.clone()),
                    Some(function(vec![e.clone()], join(n.clone(), s.clone()))),
                )
            }
            Group => {
                expected_receiver = Some(array(e.clone()));
                (
                    vec![],
                    array(record(vec![
                        ("key", j.clone(), false),
                        ("items", array(e.clone()), false),
                    ])),
                    Some(function(vec![e.clone()], j.clone())),
                )
            }
            Unique => {
                expected_receiver = Some(array(e.clone()));
                (
                    vec![],
                    array(e.clone()),
                    Some(function(vec![e.clone()], j.clone())),
                )
            }
            Sum => {
                expected_receiver = Some(array(n.clone()));
                (vec![], n.clone(), None)
            }
            Keys => {
                expected_receiver = Some(record(vec![]));
                (vec![], array(s.clone()), None)
            }
            Has => {
                expected_receiver = Some(record(vec![]));
                (vec![s.clone()], ty(Type::Bool), None)
            }
            Pick => {
                expected_receiver = Some(record(vec![]));
                (vec![array(s.clone())], record(vec![]), None)
            }
            Merge => {
                expected_receiver = Some(record(vec![]));
                (vec![record(vec![])], record(vec![]), None)
            }
            One => {
                expected_receiver = Some(array(e.clone()));
                (vec![], e.clone(), None)
            }
            Split => {
                expected_receiver = Some(s.clone());
                (vec![s.clone()], array(s.clone()), None)
            }
            Join => {
                expected_receiver = Some(array(s.clone()));
                (vec![s.clone()], s.clone(), None)
            }
            Replace => {
                expected_receiver = Some(s.clone());
                (vec![s.clone(), s.clone()], s.clone(), None)
            }
            Text => (vec![scalar()], s.clone(), None),
            Number => (vec![s.clone()], n.clone(), None),
            Parse => (vec![s.clone()], j.clone(), None),
            Dump => (vec![j.clone()], s.clone(), None),
            Encode => (vec![s.clone()], ty(Type::Bytes), None),
            Decode => (vec![ty(Type::Bytes)], s.clone(), None),
            Validate => {
                let value = generic("Value");
                (vec![value, ty(Type::Schema(r.clone()))], r.clone(), None)
            }
            Maybe => (
                vec![ty(Type::Schema(r.clone()))],
                ty(Type::Schema(join(r.clone(), null.clone()))),
                None,
            ),
            Optional => (
                vec![ty(Type::Schema(r.clone()))],
                ty(Type::OptionalSchema(r.clone())),
                None,
            ),
            Open => (
                vec![ty(Type::Schema(r.clone()))],
                ty(Type::Schema(r.clone())),
                None,
            ),
            Parallel => {
                expected_receiver = Some(array(e.clone()));
                (
                    vec![i.clone()],
                    array(r.clone()),
                    Some(function(vec![e.clone()], r.clone())),
                )
            }
            Sleep => (vec![i.clone()], null.clone(), None),
            Retry => (
                vec![record(vec![
                    ("attempts", i.clone(), false),
                    ("when", function(vec![error()], ty(Type::Bool)), false),
                    ("delay", i.clone(), false),
                ])],
                r.clone(),
                Some(function(vec![], r.clone())),
            ),
            Pages => {
                let cursor = generic("Cursor");
                (
                    vec![
                        cursor.clone(),
                        record(vec![
                            ("max_pages", i.clone(), true),
                            ("max_items", i.clone(), true),
                        ]),
                    ],
                    array(r.clone()),
                    Some(function(
                        vec![cursor.clone()],
                        record(vec![
                            ("items", array(r.clone()), false),
                            ("next", join(cursor, null.clone()), false),
                        ]),
                    )),
                )
            }
            Fail => (vec![error()], ty(Type::Never), None),
            Http => (
                vec![s.clone(), s.clone(), http_options(true)],
                record(vec![
                    ("status", i.clone(), false),
                    ("headers", dict(array(s.clone())), false),
                    ("body", ty(Type::Bytes), false),
                ]),
                None,
            ),
            Get => (vec![s.clone(), http_options(false)], j.clone(), None),
            Post => (
                vec![s.clone(), j.clone(), http_options(false)],
                j.clone(),
                None,
            ),
            Url => (
                vec![
                    s.clone(),
                    array(s.clone()),
                    dict(join(scalar(), array(scalar()))),
                ],
                s.clone(),
                None,
            ),
            Db => (vec![s.clone()], ty(Type::Database), None),
            Sql => (
                vec![
                    join(ty(Type::Database), ty(Type::Transaction)),
                    ty(Type::Query),
                    record(vec![
                        ("timeout", i.clone(), true),
                        ("max_rows", i.clone(), true),
                    ]),
                ],
                record(vec![
                    ("rows", array(dict(join(j.clone(), ty(Type::Bytes)))), false),
                    ("count", i.clone(), false),
                ]),
                None,
            ),
            Tx => (
                vec![
                    ty(Type::Database),
                    record(vec![("timeout", i.clone(), true)]),
                ],
                r.clone(),
                Some(function(vec![ty(Type::Transaction)], r.clone())),
            ),
            Serve => (
                vec![
                    dict(function(vec![request()], join(j.clone(), ty(Type::Reply)))),
                    record(vec![
                        ("host", s.clone(), true),
                        ("port", i.clone(), true),
                        ("timeout", i.clone(), true),
                        ("max_body", i.clone(), true),
                        ("max_inflight", i.clone(), true),
                    ]),
                ],
                ty(Type::Never),
                None,
            ),
            Input => (
                vec![request(), ty(Type::Schema(r.clone()))],
                r.clone(),
                None,
            ),
            Reply => (
                vec![i.clone(), join(j.clone(), ty(Type::Bytes)), dict(s.clone())],
                ty(Type::Reply),
                None,
            ),
            Env => (vec![s.clone()], s.clone(), None),
            Read => (vec![s.clone()], ty(Type::Bytes), None),
            Write => (vec![s.clone(), ty(Type::Bytes)], null.clone(), None),
            Now | Uuid => (vec![], s.clone(), None),
            Log => (vec![j.clone()], null.clone(), None),
        };
        let sig = b.signature();
        let mut f = Function {
            params,
            result,
            callback,
            min: sig.min - usize::from(receiver.is_some()),
            optional_callback: sig.callback == crate::value::CallbackPolicy::Optional,
            generics,
            builtin: Some(b),
            receiver: receiver.clone(),
        };
        // Bind receiver element variables before exposing the method's remaining generics.
        if let (Some(actual), Some(expected)) = (&receiver, &expected_receiver) {
            let mut replacements = std::collections::HashMap::new();
            for id in &f.generics {
                replacements.insert(*id, self.solver.fresh());
            }
            let want = super::solver::substitute(expected, &replacements, &Default::default());
            self.solver.require(actual, &want, span)?;
            let elem_id = if let Type::Param { id, .. } = &*e {
                Some(*id)
            } else {
                None
            };
            if let Some(id) = elem_id {
                if let Some(replacement) = replacements.get(&id) {
                    let resolved = self.solver.deep(replacement);
                    let mut substitution = std::collections::HashMap::new();
                    substitution.insert(id, resolved);
                    let updated = super::solver::substitute(
                        &ty(Type::Function(f.clone())),
                        &substitution,
                        &Default::default(),
                    );
                    if let Type::Function(updated) = &*updated {
                        f = updated.clone();
                    }
                    f.generics.retain(|p| *p != id);
                }
            }
        }
        let mut used = std::collections::HashSet::new();
        super::parameters_in(&ty(Type::Function(f.clone())), &mut used);
        f.generics.retain(|p| used.contains(p));
        Ok(ty(Type::Function(f)))
    }
}
