use linter_api::Expr;

fn main() {
    let expr1 = Expr::new(1);
    let expr2 = Expr {
        id: 10,
        child: Some(&expr1),
    };
    let expr3 = Expr {
        id: 100,
        child: Some(&expr2),
    };
    let expr4 = Expr {
        id: 1000,
        child: Some(&expr3),
    };

    println!("{expr4:#?}");
}
