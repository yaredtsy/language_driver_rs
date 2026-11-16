// B9, B10: items in an array length, a discriminant, a field type or a const's type are not looked at; a function with no name names nothing.
fn takes(a: [u8; { fn in_array_len() {} 1 }]) {}
struct F { a: [u8; { fn in_field() {} 1 }] }
enum D { A = { fn in_discriminant() {} 1 } }
type T = [u8; { fn in_alias() {} 1 }];
const U: [u8; { fn in_const_type() {} 1 }] = [0];
const : u8 = { fn lost_const() {} 1 };
fn () { fn lost() {} }
