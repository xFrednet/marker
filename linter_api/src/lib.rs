use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Expr<'ast> {
    pub id: u32,
    pub child: Option<&'ast Expr<'ast>>,
}

impl<'ast> Expr<'ast> {
    pub fn new(id: u32) -> Self {
        Self { id, child: None }
    }
}
