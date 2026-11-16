// P8: a braced const argument and a macro in a type follow P1 to P7 like every other token, so spacing never renames a def.
impl T for S<{ N+1 }> { fn a() {} }
impl T for S<{N + 1}> { fn b() {} }
impl T for S<{ N + /* c */ 1 }> { fn c() {} }
impl T for S<3> { fn d() {} }
impl T for S<-1> { fn e() {} }
impl T for S<true> { fn f() {} }
impl T for S<'x'> { fn g() {} }
impl T for m!( a  b ) { fn h() {} }
impl T for m!(a b) { fn i() {} }
impl T for m!{ a, b } { fn j() {} }
impl T for m!(a,) { fn k() {} }
impl T for m!('a x) { fn l() {} }
