"""Step 01.04: imports and modules. ty resolves every import; the walker only
has to dispatch an attribute on the receiver's value."""

import pkg.sub.mod
import util
import util as u
from pkg.sub import mod
from util import helper
from util import helper as h2


def a():
    pass


def direct():
    helper()


def via_module():
    util.helper()


def via_alias():
    u.helper()
    h2()


def deep():
    pkg.sub.mod.work()


def from_package():
    mod.work()


def module_in_a_local():
    m = util
    m.helper()


def local_import():
    other = a
    from util import other

    other()


# expect:
# direct
#   util.helper
# expect:
# via_module
#   util.helper
# expect:
# via_alias
#   util.helper
#   util.helper
# expect:
# deep
#   pkg.sub.mod.work
# expect:
# from_package
#   pkg.sub.mod.work
# expect:
# module_in_a_local
#   util.helper
# expect:
# local_import
#   util.other
