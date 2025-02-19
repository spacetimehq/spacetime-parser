#[cfg(feature = "bindings")]
mod bindings;
pub mod compiler;
pub mod js;
pub mod stableast;
mod validation;

use polylang_parser::{LexicalError, ParseError};
use serde::Serialize;
use std::collections::HashMap;

pub use polylang_parser::ast;

#[derive(Debug, Serialize)]
pub struct Error {
    pub message: String,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for Error {}

fn parse_error_to_error<T>(input: &str, error: ParseError<usize, T, LexicalError>) -> Error
where
    T: std::fmt::Display + std::fmt::Debug,
{
    let get_line_start = |start_byte| input[..start_byte].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let get_line_end = |end_byte| {
        input[end_byte..]
            .find('\n')
            .map(|i| i + end_byte)
            .unwrap_or_else(|| input.len())
    };

    let make_err = |start_byte, end_byte, message: &str| {
        let line_start = get_line_start(start_byte);
        let line_end = get_line_end(end_byte);
        let line = &input[line_start..line_end];
        let line_number = input[..start_byte].matches('\n').count() + 1;
        let column = start_byte - line_start;
        let mut message = format!(
            "Error found at line {}, column {}: {}\n",
            line_number, column, message
        );

        // deindent the line
        let line_len_before_trim = line.len();
        let line = line.trim_start();
        let column = column - (line_len_before_trim - line.len());

        message.push_str(line);
        message.push('\n');
        message.push_str(&" ".repeat(column));
        message.push_str(&"^".repeat(if start_byte == end_byte {
            1
        } else {
            end_byte - start_byte
        }));
        Error { message }
    };

    match error {
        ParseError::InvalidToken { location } => make_err(location, location, "Invalid token"),
        ParseError::UnrecognizedEOF {
            location,
            expected: _,
        } => make_err(location, location, "Unexpected end of file"),
        ParseError::UnrecognizedToken {
            token: (start_byte, token, end_byte),
            expected,
        } => make_err(
            start_byte,
            end_byte,
            &format!(
                "Unrecognized token \"{}\". Expected one of: {}",
                token,
                expected.join(", "),
            ),
        ),
        ParseError::ExtraToken {
            token: (start_byte, token, end_byte),
        } => make_err(start_byte, end_byte, &format!("Extra token \"{}\"", token)),
        ParseError::User { error } => match error {
            LexicalError::NumberParseError { start, end } => {
                make_err(start, end, "Failed to parse number")
            }
            LexicalError::InvalidToken { start, end } => make_err(start, end, "Invalid token"),
            LexicalError::UnterminatedComment { start, end } => {
                make_err(start, end, "Unterminated comment")
            }
            LexicalError::UnterminatedString { start, end } => {
                make_err(start, end, "Unterminated string")
            }
            LexicalError::UserError {
                start,
                end,
                message,
            } => make_err(start, end, &message),
        },
    }
}

pub fn parse_program(input: &str) -> Result<ast::Program, Error> {
    polylang_parser::parse(input).map_err(|e| parse_error_to_error(input, e))
}

pub fn parse<'a>(
    input: &'a str,
    namespace: &'a str,
    program_holder: &'a mut Option<ast::Program>,
) -> Result<(&'a ast::Program, stableast::Root<'a>), Error> {
    program_holder.replace(parse_program(input)?);

    Ok((
        program_holder.as_ref().unwrap(),
        stableast::Root::from_ast(namespace, program_holder.as_ref().unwrap())
            .map_err(|e| Error { message: e })?,
    ))
}

fn parse_out_json(input: &str, namespace: &str) -> String {
    serde_json::to_string(&parse(input, namespace, &mut None)).unwrap()
}

fn validate_set(contract_ast_json: &str, data_json: &str) -> Result<(), Error> {
    let contract_ast: stableast::Contract = match serde_json::from_str(contract_ast_json) {
        Ok(ast) => ast,
        Err(err) => {
            return Err(Error {
                message: err.to_string(),
            })
        }
    };

    let data: HashMap<String, validation::Value> = match serde_json::from_str(data_json) {
        Ok(data) => data,
        Err(err) => {
            return Err(Error {
                message: err.to_string(),
            })
        }
    };

    validation::validate_set(&contract_ast, &data).map_err(|e| Error {
        message: e.to_string(),
    })
}

fn validate_set_out_json(contract_ast_json: &str, data_json: &str) -> String {
    serde_json::to_string(&validate_set(contract_ast_json, data_json)).unwrap()
}

fn generate_contract_function(contract_ast: &str) -> Result<js::JSContract, Error> {
    let contract_ast: stableast::Contract =
        serde_json::from_str(contract_ast).map_err(|e| Error {
            message: e.to_string(),
        })?;

    Ok(js::generate_js_contract(&contract_ast))
}

fn generate_js_contract_out_json(contract_ast: &str) -> String {
    serde_json::to_string(&generate_contract_function(contract_ast)).unwrap()
}

#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::*;

    #[test]
    fn test_parse() {
        let input = "contract Test {}";
        let expected_output = expect![[
            r#"{"Ok":[{"nodes":[{"Contract":{"name":"Test","decorators":[],"items":[]}}]},[{"kind":"contract","namespace":{"kind":"namespace","value":""},"name":"Test","attributes":[]}]]}"#
        ]];

        let output = parse_out_json(input, "");
        expected_output.assert_eq(&output);
    }

    #[test]
    fn test_parse_contract() {
        let input = "contract Test {}";
        let expected_output = expect![[
            r#"{"Ok":[{"nodes":[{"Contract":{"name":"Test","decorators":[],"items":[]}}]},[{"kind":"contract","namespace":{"kind":"namespace","value":""},"name":"Test","attributes":[]}]]}"#
        ]];

        let output = parse_out_json(input, "");
        expected_output.assert_eq(&output);
    }

    #[test]
    fn test_parse_contract_metadata() {
        let input = "@public contract Contract { id: string; name?: string; lastRecordUpdated?: string; code?: string; ast?: string; publicKey?: PublicKey; @index(publicKey); @index([lastRecordUpdated, desc]); constructor (id: string, code: string) { this.id = id; this.code = code; this.ast = parse(code); if (ctx.publicKey) this.publicKey = ctx.publicKey; } updateCode (code: string) { if (this.publicKey != ctx.publicKey) { throw error('invalid owner'); } this.code = code; this.ast = parse(code); } }";
        let expected_output = expect![[
            r#"[{"kind":"contract","namespace":{"kind":"namespace","value":""},"name":"Contract","attributes":[{"kind":"property","name":"id","type":{"kind":"primitive","value":"string"},"directives":[],"required":true},{"kind":"property","name":"name","type":{"kind":"primitive","value":"string"},"directives":[],"required":false},{"kind":"property","name":"lastRecordUpdated","type":{"kind":"primitive","value":"string"},"directives":[],"required":false},{"kind":"property","name":"code","type":{"kind":"primitive","value":"string"},"directives":[],"required":false},{"kind":"property","name":"ast","type":{"kind":"primitive","value":"string"},"directives":[],"required":false},{"kind":"property","name":"publicKey","type":{"kind":"publickey"},"directives":[],"required":false},{"kind":"index","fields":[{"direction":"asc","fieldPath":["publicKey"]}]},{"kind":"index","fields":[{"direction":"desc","fieldPath":["lastRecordUpdated"]}]},{"kind":"method","name":"constructor","attributes":[{"kind":"parameter","name":"id","type":{"kind":"primitive","value":"string"},"required":true},{"kind":"parameter","name":"code","type":{"kind":"primitive","value":"string"},"required":true}],"code":"this.id = id; this.code = code; this.ast = parse(code); if (ctx.publicKey) this.publicKey = ctx.publicKey;"},{"kind":"method","name":"updateCode","attributes":[{"kind":"parameter","name":"code","type":{"kind":"primitive","value":"string"},"required":true}],"code":"if (this.publicKey != ctx.publicKey) { throw error('invalid owner'); } this.code = code; this.ast = parse(code);"},{"kind":"directive","name":"public","arguments":[]}]}]"#
        ]];

        let mut program = None::<ast::Program>;
        let output = parse(input, "", &mut program).unwrap().1;
        let output = serde_json::to_string(&output).unwrap();

        expected_output.assert_eq(&output);
    }

    #[test]
    fn test_contract() {
        let mut program = None::<ast::Program>;
        let (program, _) = parse("contract Test {}", "", &mut program).unwrap();

        assert_eq!(program.nodes.len(), 1);
        assert!(
            matches!(&program.nodes[0], ast::RootNode::Contract(ast::Contract { name, decorators, items }) if name == "Test" && decorators.is_empty() && items.is_empty())
        );
    }

    #[test]
    fn test_contract_with_fields() {
        let mut program = None::<ast::Program>;
        let (program, _) = parse(
            "
            contract Test {
                name: string;
                age: number;
            }
            ",
            "",
            &mut program,
        )
        .unwrap();

        assert_eq!(program.nodes.len(), 1);
        assert!(
            matches!(&program.nodes[0], ast::RootNode::Contract(ast::Contract { name, decorators, items }) if name == "Test" && decorators.is_empty() && items.len() == 2)
        );

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(contract) => contract,
            _ => panic!("Expected contract"),
        };

        assert!(
            matches!(&contract.items[0], ast::ContractItem::Field(ast::Field { name, type_, required: true, decorators }) if name == "name" && *type_ == ast::Type::String && decorators.is_empty())
        );
        assert!(
            matches!(&contract.items[1], ast::ContractItem::Field(ast::Field { name, type_, required: true, decorators }) if name == "age" && *type_ == ast::Type::Number && decorators.is_empty())
        );
    }

    #[test]
    fn test_contract_with_asc_desc_fields() {
        let mut program = None::<ast::Program>;
        let (program, _) = parse(
            "
            contract Test {
                asc: string;
                desc: string;
            }
            ",
            "",
            &mut program,
        )
        .unwrap();

        assert_eq!(program.nodes.len(), 1);
        assert!(
            matches!(&program.nodes[0], ast::RootNode::Contract(ast::Contract { name, decorators, items }) if name == "Test" && decorators.is_empty() && items.len() == 2)
        );

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(contract) => contract,
            _ => panic!("Expected contract"),
        };

        assert!(
            matches!(&contract.items[0], ast::ContractItem::Field(ast::Field { name, type_, required: true, decorators }) if name == "asc" && *type_ == ast::Type::String && decorators.is_empty()),
        );
        assert!(
            matches!(&contract.items[1], ast::ContractItem::Field(ast::Field { name, type_, required: true, decorators }) if name == "desc" && *type_ == ast::Type::String && decorators.is_empty()),
        );
    }

    #[test]
    fn test_contract_with_functions() {
        let mut program = None::<ast::Program>;
        let (program, _) = parse(
            "
            contract Test {
                function get_age(a: number, b?: string) {
                    return 42;
                }
            }
            ",
            "",
            &mut program,
        )
        .unwrap();

        assert_eq!(program.nodes.len(), 1);
        assert!(
            matches!(&program.nodes[0], ast::RootNode::Contract(ast::Contract { name, decorators, items }) if name == "Test" && decorators.is_empty() && items.len() == 1)
        );

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(contract) => contract,
            _ => panic!("Expected contract"),
        };

        assert!(
            matches!(&contract.items[0], ast::ContractItem::Function(ast::Function {
                name,
                decorators,
                parameters,
                statements,
                statements_code,
                return_type,
            }) if name == "get_age" && decorators.is_empty() && parameters.len() == 2 && statements.len() == 1 && statements_code == "return 42;" && return_type.is_none())
        );

        let function = match &contract.items[0] {
            ast::ContractItem::Function(function) => function,
            _ => panic!("Expected function"),
        };

        assert!(
            matches!(&*function.statements[0], ast::StatementKind::Return(ref expr) if matches!(**expr, ast::ExpressionKind::Primitive(ast::Primitive::Number(number, has_decimal_point)) if number == 42.0 && !has_decimal_point))
        );
        assert!(
            matches!(&function.parameters[0], ast::Parameter{ name, type_, required } if *required && name == "a" && *type_ == ast::ParameterType::Number)
        );
        assert!(
            matches!(&function.parameters[1], ast::Parameter{ name, type_, required } if !(*required) && name == "b" && *type_ == ast::ParameterType::String)
        );
    }

    #[test]
    fn test_number() {
        let number = polylang_parser::parse_expression("42");

        assert!(number.is_ok());
        assert_eq!(
            &*number.unwrap(),
            &ast::ExpressionKind::Primitive(ast::Primitive::Number(42.0, false))
        );
    }

    #[test]
    fn test_number_decimal() {
        let number = polylang_parser::parse_expression("42.0");

        assert!(number.is_ok());
        assert_eq!(
            *number.unwrap(),
            ast::ExpressionKind::Primitive(ast::Primitive::Number(42.0, true))
        );
    }

    #[test]
    fn test_number_decimal_2() {
        let number = polylang_parser::parse_expression("42.5");

        assert!(number.is_ok());
        assert_eq!(
            *number.unwrap(),
            ast::ExpressionKind::Primitive(ast::Primitive::Number(42.5, true))
        );
    }

    #[test]
    fn test_string_single() {
        let string = polylang_parser::parse_expression("'hello\" world'");

        assert!(string.is_ok());
        assert_eq!(
            *string.unwrap(),
            ast::ExpressionKind::Primitive(ast::Primitive::String("hello\" world".to_string()))
        );
    }

    #[test]
    fn test_string_double() {
        let string = polylang_parser::parse_expression("\"hello' world\"");

        assert!(string.is_ok());
        assert_eq!(
            *string.unwrap(),
            ast::ExpressionKind::Primitive(ast::Primitive::String("hello' world".to_string()))
        );
    }

    #[test]
    fn test_comparison() {
        let comparison = polylang_parser::parse_expression("1 > 2");

        assert_eq!(
            *comparison.unwrap(),
            ast::ExpressionKind::GreaterThan(
                Box::new(ast::ExpressionKind::Primitive(ast::Primitive::Number(1.0, false)).into()),
                Box::new(ast::ExpressionKind::Primitive(ast::Primitive::Number(2.0, false)).into()),
            )
        );
    }

    // #[test]
    // fn test_if() {
    //     let mut program = None::<ast::Program>;
    //     let (_, _) = parse(
    //         "
    //         function x() {
    //             if (1 == 1) {
    //                 return 42;
    //             } else {
    //                 return 0;
    //             }
    //         }
    //         ",
    //         "",
    //         &mut None,
    //     )
    //     .unwrap();

    //     assert_eq!(program.nodes.len(), 1);

    //     let mut function = match program.nodes.pop().unwrap() {
    //         ast::RootNode::Function(function) => function,
    //         _ => panic!("Expected function"),
    //     };

    //     assert_eq!(function.statements.len(), 1);

    //     let if_ = match function.statements.pop().unwrap() {
    //         ast::Statement::If(if_) => if_,
    //         _ => panic!("Expected if"),
    //     };

    //     assert!(
    //         matches!(if_.condition, ast::ExpressionKind::Equal(n, m) if *n == ast::ExpressionKind::Primitive(ast::Primitive::Number(1.0)) && *m == ast::ExpressionKind::Primitive(ast::Primitive::Number(1.0)))
    //     );
    //     assert_eq!(if_.then_statements.len(), 1);
    //     assert_eq!(if_.else_statements.len(), 1);
    // }

    #[test]
    fn test_call() {
        let call = polylang_parser::parse_expression("get_age(a, b, c)");

        assert!(matches!(
            &*call.unwrap(),
            ast::ExpressionKind::Call(f, args) if ***f == ast::ExpressionKind::Ident("get_age".to_owned()) && args.len() == 3
        ));
    }

    #[test]
    fn test_dot() {
        let dot = polylang_parser::parse_expression("a.b").unwrap();

        assert_eq!(
            &*dot,
            &ast::ExpressionKind::Dot(
                Box::new(ast::ExpressionKind::Ident("a".to_owned()).into()),
                "b".to_string()
            )
        );
    }

    #[test]
    fn test_assign_sub() {
        let dot = polylang_parser::parse_expression("a -= b").unwrap();

        assert_eq!(
            &*dot,
            &ast::ExpressionKind::AssignSub(
                Box::new(ast::ExpressionKind::Ident("a".to_owned()).into()),
                Box::new(ast::ExpressionKind::Ident("b".to_owned()).into())
            )
        );
    }

    #[test]
    fn test_code_from_issue() {
        let code = "
            contract Account {
                name: string;
                age?: number;
                balance: number;
                publicKey: string;
            
                @index([field, asc], field2);
            
                transfer (b: record, amount: number) {
                    if (this.publicKey != $auth.publicKey) throw error('invalid user');
                    
                    this.balance -= amount;
                    b.balance += amount;
                }
            }
        ";

        let mut program = None::<ast::Program>;
        let (program, _) = parse(code, "", &mut program).unwrap();

        assert_eq!(program.nodes.len(), 1);

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(contract) => contract,
            _ => panic!("Expected contract"),
        };

        assert_eq!(contract.name, "Account");
        assert_eq!(contract.items.len(), 6);

        assert!(matches!(
            &contract.items[0],
            ast::ContractItem::Field(ast::Field { name, type_, required: true, decorators })
            if name == "name" && *type_ == ast::Type::String && decorators.is_empty()
        ));

        assert!(matches!(
            &contract.items[1],
            ast::ContractItem::Field(ast::Field { name, type_, required: false, decorators })
            if name == "age" && *type_ == ast::Type::Number && decorators.is_empty()
        ));

        assert!(matches!(
            &contract.items[2],
            ast::ContractItem::Field(ast::Field { name, type_, required: true, decorators })
            if name == "balance" && *type_ == ast::Type::Number && decorators.is_empty()
        ));

        assert!(matches!(
            &contract.items[3],
            ast::ContractItem::Field(ast::Field { name, type_, required: true, decorators })
            if name == "publicKey" && *type_ == ast::Type::String && decorators.is_empty()
        ));

        assert!(matches!(
            &contract.items[4],
            ast::ContractItem::Index(ast::Index {
                fields,
            }) if fields[0].path == ["field"] && fields[0].order == ast::Order::Asc
                && fields[1].path == ["field2"] && fields[1].order == ast::Order::Asc
        ));

        let function = match &contract.items[5] {
            ast::ContractItem::Function(f) => f,
            _ => panic!("expected function"),
        };
        dbg!(&function.statements);

        assert!(matches!(
            &*function.statements[0],
            ast::StatementKind::If(ast::If {
                condition,
                then_statements,
                else_statements,
            }) if **condition == ast::ExpressionKind::NotEqual(
                Box::new(ast::ExpressionKind::Dot(
                    Box::new(ast::ExpressionKind::Ident("this".to_owned()).into()),
                    "publicKey".to_owned(),
                ).into()),
                Box::new(ast::ExpressionKind::Dot(
                    Box::new(ast::ExpressionKind::Ident("$auth".to_owned()).into()),
                    "publicKey".to_owned(),
                ).into()),
            ) && then_statements.len() == 1 && else_statements.is_empty()
        ));

        assert!(matches!(
            &*function.statements[1],
            ast::StatementKind::Expression(expr)
            if matches!(&**expr, ast::ExpressionKind::AssignSub(
                left,
                right,
            ) if ***left == ast::ExpressionKind::Dot(
                Box::new(ast::ExpressionKind::Ident("this".to_owned()).into()),
                "balance".to_owned(),
            ) && ***right == ast::ExpressionKind::Ident("amount".to_owned())
        )));

        assert!(matches!(
            &*function.statements[2],
            ast::StatementKind::Expression(expr)
            if matches!(&**expr, ast::ExpressionKind::AssignAdd(
                left,
                right,
            ) if matches!(&***left, ast::ExpressionKind::Dot(
                expr,
                v,
            ) if v == "balance" && ***expr == ast::ExpressionKind::Ident("b".to_owned())) && ***right == ast::ExpressionKind::Ident("amount".to_owned())
        )));
    }

    //     #[test]
    //     fn test_generate_js_function() {
    //         let func_code = "
    //             function transfer (a: record, b: record, amount: number) {
    //                 if (a.publicKey != $auth.publicKey) throw error('invalid user');

    //                 a.balance -= amount;
    //                 b.balance += amount;
    //             }
    //         ";

    //         let func = polylang::FunctionParser::new().parse(func_code).unwrap();
    //         let func_ast = serde_json::to_string(&func).unwrap();

    //         let eval_input = generate_js_function(&func_ast).unwrap();
    //         assert_eq!(
    //             eval_input,
    //             JSFunc {
    //                 code: "
    // function error(str) {
    //     return new Error(str);
    // }

    // const f = ($auth, args) => {
    // const a = args[0], b = args[1], amount = args[2];
    // if (a.publicKey != $auth.publicKey) throw error('invalid user');

    //                 a.balance -= amount;
    //                 b.balance += amount;
    // };
    // "
    //                 .to_string(),
    //             },
    //         );
    //     }

    #[test]
    fn test_error_unrecognized_token() {
        let code = "
            contract test-cities {}
        ";

        let mut program = None::<ast::Program>;
        let result = parse(code, "", &mut program);
        assert!(result.is_err());
        eprintln!("{}", result.as_ref().unwrap_err().message);
        assert_eq!(
            result.unwrap_err().message,
            r#"Error found at line 2, column 25: Unrecognized token "-". Expected one of: "{"
contract test-cities {}
             ^"#,
        );
    }

    #[test]
    fn test_error_invalid_token() {
        let code = "
            contract ą {}
        ";

        let mut program = None::<ast::Program>;
        let result = parse(code, "", &mut program);
        assert!(result.is_err());
        eprintln!("{}", result.as_ref().unwrap_err().message);
        assert_eq!(
            result.unwrap_err().message,
            r#"Error found at line 2, column 21: Invalid token
contract ą {}
         ^"#,
        );
    }

    #[test]
    fn test_error_unexpected_eof() {
        let code = "
            function x() {
        ";

        let mut program = None::<ast::Program>;
        let result = parse(code, "", &mut program);
        assert!(result.is_err());
        eprintln!("{}", result.as_ref().unwrap_err().message);
        assert_eq!(
            result.unwrap_err().message,
            r#"Error found at line 2, column 26: Unexpected end of file
function x() {
              ^"#,
        );
    }

    #[test]
    fn test_foreign_record_field() {
        let code = "
            contract test {
                account: Account;
            }
        ";

        let mut program = None::<ast::Program>;
        let (program, _) = parse(code, "", &mut program).unwrap();

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(c) => c,
            _ => panic!("expected contract"),
        };

        assert_eq!(contract.items.len(), 1);

        let field = match &contract.items[0] {
            ast::ContractItem::Field(f) => f,
            _ => panic!("expected field"),
        };

        assert_eq!(field.name, "account");
        assert_eq!(
            field.type_,
            ast::Type::ForeignRecord {
                contract: "Account".to_string(),
            }
        );
    }

    #[test]
    fn test_array_map_field() {
        let cases = [
            (
                "contract test { numbers: number[]; }",
                vec![ast::Field {
                    name: "numbers".to_string(),
                    type_: ast::Type::Array(Box::new(ast::Type::Number)),
                    required: true,
                    decorators: vec![],
                }],
            ),
            (
                "contract test { strings: string[]; }",
                vec![ast::Field {
                    name: "strings".to_string(),
                    type_: ast::Type::Array(Box::new(ast::Type::String)),
                    required: true,
                    decorators: vec![],
                }],
            ),
            (
                "contract test { numToStr: map<number, string>; }",
                vec![ast::Field {
                    name: "numToStr".to_string(),
                    type_: ast::Type::Map(Box::new(ast::Type::Number), Box::new(ast::Type::String)),
                    required: true,
                    decorators: vec![],
                }],
            ),
            (
                "contract test { strToNum: map<string, number>; }",
                vec![ast::Field {
                    name: "strToNum".to_string(),
                    type_: ast::Type::Map(Box::new(ast::Type::String), Box::new(ast::Type::Number)),
                    required: true,
                    decorators: vec![],
                }],
            ),
        ];

        for (code, expected) in cases.iter() {
            let mut program = None::<ast::Program>;
            let (program, _) = parse(code, "", &mut program).unwrap();
            assert_eq!(program.nodes.len(), 1);
            let contract = match &program.nodes[0] {
                ast::RootNode::Contract(c) => c,
                _ => panic!("expected contract"),
            };
            assert_eq!(contract.items.len(), expected.len());

            for (i, item) in expected.iter().enumerate() {
                assert!(
                    matches!(
                        &contract.items[i],
                        ast::ContractItem::Field(ast::Field {
                            name,
                            type_,
                            required,
                            decorators,
                        }) if name == &item.name && type_ == &item.type_ && required == &item.required && decorators == &item.decorators
                    ),
                    "expected: {:?}, got: {:?}",
                    item,
                    contract.items[i]
                );
            }
        }
    }

    #[test]
    fn test_object_field() {
        let cases = [
            (
                "contract test { person: { name: string; age: number; }; }",
                vec![ast::Field {
                    name: "person".to_string(),
                    type_: ast::Type::Object(vec![
                        ast::Field {
                            name: "name".to_string(),
                            type_: ast::Type::String,
                            required: true,
                            decorators: vec![],
                        },
                        ast::Field {
                            name: "age".to_string(),
                            type_: ast::Type::Number,
                            required: true,
                            decorators: vec![],
                        },
                    ]),
                    required: true,
                    decorators: vec![],
                }],
            ),
            (
                "contract test { person: { name?: string; }; }",
                vec![ast::Field {
                    name: "person".to_string(),
                    type_: ast::Type::Object(vec![ast::Field {
                        name: "name".to_string(),
                        type_: ast::Type::String,
                        required: false,
                        decorators: vec![],
                    }]),
                    required: true,
                    decorators: vec![],
                }],
            ),
            (
                "contract test { person: { info: { name: string; }; }; }",
                vec![ast::Field {
                    name: "person".to_string(),
                    type_: ast::Type::Object(vec![ast::Field {
                        name: "info".to_string(),
                        type_: ast::Type::Object(vec![ast::Field {
                            name: "name".to_string(),
                            type_: ast::Type::String,
                            required: true,
                            decorators: vec![],
                        }]),
                        required: true,
                        decorators: vec![],
                    }]),
                    required: true,
                    decorators: vec![],
                }],
            ),
        ];

        for (code, expected) in cases.iter() {
            let mut program = None::<ast::Program>;
            let (program, _) = parse(code, "", &mut program).unwrap();
            assert_eq!(program.nodes.len(), 1);
            let contract = match &program.nodes[0] {
                ast::RootNode::Contract(c) => c,
                _ => panic!("expected contract"),
            };
            assert_eq!(contract.items.len(), expected.len());

            for (i, item) in expected.iter().enumerate() {
                assert!(
                    matches!(
                        &contract.items[i],
                        ast::ContractItem::Field(ast::Field {
                            name,
                            type_,
                            required,
                            decorators,
                        }) if name == &item.name && type_ == &item.type_ && required == &item.required && decorators.is_empty()
                    ),
                    "expected: {:?}, got: {:?}",
                    item,
                    contract.items[i]
                );
            }
        }
    }

    #[test]
    fn test_comments() {
        let code = "
            contract test {
                // This is a comment
                name: string;

                /*
                    This is a multiline comment
                */
                function test() {
                    return 1;
                }
            }
        ";

        assert!(parse(code, "", &mut None).is_ok());
    }

    #[test]
    fn test_index_subfield() {
        let code = "
            contract test {
                person: {
                    name: string;
                };

                @index(person.name);
            }
        ";

        let mut program = None::<ast::Program>;
        let (program, _) = parse(code, "", &mut program).unwrap();
        assert_eq!(program.nodes.len(), 1);

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(c) => c,
            _ => panic!("expected contract"),
        };
        assert_eq!(contract.items.len(), 2);

        assert!(
            matches!(
                &contract.items[1],
                ast::ContractItem::Index(ast::Index {
                    fields,
                }) if fields == &[ast::IndexField { path: vec!["person".to_string(), "name".to_string()], order: ast::Order::Asc }]
            ),
            "expected: {:?}, got: {:?}",
            &contract.items[1],
            &contract.items[1]
        );
    }

    #[test]
    fn test_decorators() {
        let code = "
                @public
                contract Account {
                @read
                owner: PublicKey;

                @call(owner)
                function noop() {}
            }
        ";

        let mut program = None::<ast::Program>;
        let (program, _) = parse(code, "", &mut program).unwrap();
        assert_eq!(program.nodes.len(), 1);

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(c) => c,
            _ => panic!("expected contract"),
        };

        assert_eq!(contract.decorators.len(), 1);
        assert_eq!(contract.decorators[0].name, "public");

        assert_eq!(contract.items.len(), 2);

        let field = match &contract.items[0] {
            ast::ContractItem::Field(f) => f,
            _ => panic!("expected field"),
        };

        assert_eq!(field.decorators.len(), 1);
        assert_eq!(field.decorators[0].name, "read");

        let function = match &contract.items[1] {
            ast::ContractItem::Function(f) => f,
            _ => panic!("expected function"),
        };

        assert_eq!(function.decorators.len(), 1);
        assert_eq!(function.decorators[0].name, "call");
        assert_eq!(function.decorators[0].arguments.len(), 1);
        assert_eq!(
            function.decorators[0].arguments[0],
            ast::DecoratorArgument::Identifier("owner".to_owned()),
        );
    }

    #[test]
    fn test_foreign_record_array() {
        let code = "
            contract test {
                people: Person[];
            }
        ";

        let mut program = None;
        let (program, _) = parse(code, "", &mut program).unwrap();
        assert_eq!(program.nodes.len(), 1);

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(c) => c,
            _ => panic!("expected contract"),
        };

        assert_eq!(contract.items.len(), 1);

        let field = match &contract.items[0] {
            ast::ContractItem::Field(f) => f,
            _ => panic!("expected field"),
        };

        assert_eq!(field.name, "people");
        assert_eq!(
            field.type_,
            ast::Type::Array(Box::new(ast::Type::ForeignRecord {
                contract: "Person".to_string()
            }))
        );
    }

    #[test]
    fn test_expr_array_empty() {
        let code = "[]";
        let expr = polylang_parser::parse_expression(code).unwrap();
        assert_eq!(*expr, ast::ExpressionKind::Array(vec![]));
    }

    #[test]
    fn test_expr_array_single() {
        let code = "[1]";
        let expr = polylang_parser::parse_expression(code).unwrap();
        assert_eq!(
            *expr,
            ast::ExpressionKind::Array(vec![ast::ExpressionKind::Primitive(
                ast::Primitive::Number(1.0, false)
            )
            .into()])
        );
    }

    #[test]
    fn test_expr_array_multiple() {
        let code = "[1, 2, 3]";
        let expr = polylang_parser::parse_expression(code).unwrap();
        assert_eq!(
            *expr,
            ast::ExpressionKind::Array(vec![
                ast::ExpressionKind::Primitive(ast::Primitive::Number(1.0, false)).into(),
                ast::ExpressionKind::Primitive(ast::Primitive::Number(2.0, false)).into(),
                ast::ExpressionKind::Primitive(ast::Primitive::Number(3.0, false)).into(),
            ])
        );
    }

    #[test]
    fn test_expr_array_nested() {
        let code = "[[1], [2, 3]]";
        let expr = polylang_parser::parse_expression(code).unwrap();
        assert_eq!(
            *expr,
            ast::ExpressionKind::Array(vec![
                ast::ExpressionKind::Array(vec![ast::ExpressionKind::Primitive(
                    ast::Primitive::Number(1.0, false)
                )
                .into()])
                .into(),
                ast::ExpressionKind::Array(vec![
                    ast::ExpressionKind::Primitive(ast::Primitive::Number(2.0, false)).into(),
                    ast::ExpressionKind::Primitive(ast::Primitive::Number(3.0, false)).into(),
                ])
                .into(),
            ])
        );
    }

    #[test]
    fn test_expr_object_empty() {
        let code = "{}";
        let expr = polylang_parser::parse_expression(code).unwrap();
        assert_eq!(
            *expr,
            ast::ExpressionKind::Object(ast::Object { fields: vec![] })
        );
    }

    #[test]
    fn test_object() {
        let code = r#"
            contract Test {
                field: { a?: number; };

                constructor () {
                    self.field = {};
                }
            }
        "#;

        let mut program = None;
        let (program, _) = parse(code, "", &mut program).unwrap();
        assert_eq!(program.nodes.len(), 1);

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(c) => c,
            _ => panic!("expected contract"),
        };

        assert_eq!(contract.items.len(), 2);

        let field = match &contract.items[0] {
            ast::ContractItem::Field(f) => f,
            _ => panic!("expected field"),
        };

        assert_eq!(field.name, "field");
        assert_eq!(
            field.type_,
            ast::Type::Object(vec![ast::Field {
                name: "a".to_string(),
                type_: ast::Type::Number,
                required: false,
                decorators: vec![],
            }])
        );

        let function = match &contract.items[1] {
            ast::ContractItem::Function(f) => f,
            _ => panic!("expected function"),
        };

        assert_eq!(function.name, "constructor");
        assert_eq!(function.parameters.len(), 0);

        let (_l, r) = match &*function.statements[0] {
            ast::StatementKind::Expression(e) => match &**e {
                ast::ExpressionKind::Assign(l, r) => (l, r),
                _ => panic!("expected assignment"),
            },
            _ => panic!("expected assignment"),
        };

        assert_eq!(
            ***r,
            ast::ExpressionKind::Object(ast::Object { fields: vec![] })
        );
    }

    #[test]
    fn test_public_key_array_decl() {
        let code = r#"
            contract PublicKeyArrayDemo {
              publicKeys: PublicKey[];

              constructor () {
                this.publicKeys = [];
                this.publicKeys.push(ctx.publicKey);
                this.publicKeys.push(ctx.publicKey);
              }
            }
        "#;

        let mut program = None;
        let (program, _) = parse(code, "", &mut program).unwrap();
        assert_eq!(program.nodes.len(), 1);

        let contract = match &program.nodes[0] {
            ast::RootNode::Contract(c) => c,
            _ => panic!("expected contract"),
        };

        assert_eq!(contract.items.len(), 2);

        let field = match &contract.items[0] {
            ast::ContractItem::Field(f) => f,
            _ => panic!("expected publicKeys"),
        };

        assert_eq!(field.name, "publicKeys");
        assert_eq!(
            field.type_,
            ast::Type::Array(Box::new(ast::Type::PublicKey))
        );
    }
}
