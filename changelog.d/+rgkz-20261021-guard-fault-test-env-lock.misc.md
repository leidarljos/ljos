Guard-fault relay tests now clear and restore `LJOS_GUARD_FAIL_OPEN`
under the test environment lock, so parallel test runs cannot leak the
fail-open switch into fail-closed assertions.
