/**
 * A decorated class.
 * ID: dec-1
 */
@sealed
@tag("x")
export class Decorated {
  /**
   * A decorated method.
   */
  @log
  method() {}
}

@onlyDecorator
class NoDoc {}
