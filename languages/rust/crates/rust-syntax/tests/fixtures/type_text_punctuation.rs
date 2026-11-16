// P4 to P6: optional punctuation goes (the :: before <, a trailing comma, r#), the comma of a one-element tuple stays.
impl T<Vec::<u8>> for S { fn m() {} }
impl T<HashMap<K,V,>> for S { fn m() {} }
impl T<(A, B,)> for S { fn m() {} }
impl T<(A,)> for S { fn m() {} }
impl T<fn(A,)> for S { fn m() {} }
impl T<Box<dyn Fn(A, B,)>> for S { fn m() {} }
impl T<::std::vec::Vec::<u8>> for S { fn m() {} }
impl T<r#type> for S { fn m() {} }
impl r#type { fn m() {} }
impl<'a> T for r#struct<'a> { fn m() {} }
