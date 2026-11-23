import Base

def sin(x: F32) -> IO(F32):
  import "./sin.c"
  import "./sin.js"

def cos(x: F32) -> IO(F32): import "./cos.c"
