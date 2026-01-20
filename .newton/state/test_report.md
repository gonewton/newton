# Newton Test Results Report
Generated: 2026-01-20T14:57:10Z
Command: ./scripts/run-tests.sh
Output File: ./.newton/state/test_report.md
JSON File: ./.newton/state/test_output.json

## Overall Status
❌ **COMPILATION FAILED** - Code does not compile, tests cannot run

## Test Statistics
- **Status:** Compilation failed - no tests executed
- **Total Tests:** N/A
- **Passed:** N/A
- **Failed:** N/A
- **Skipped:** N/A
- **Passing Rate:** N/A

## Progress Visualization
```
[░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░] COMPILATION FAILED
```

## Files
- **Raw Test Output:** `./.newton/state/test_output.json`
- **Markdown Report:** `./.newton/state/test_report.md`

## Raw Test Output
Complete test output is saved in: `./.newton/state/test_output.json`

You can analyze it with standard Unix tools:
```bash
# Count total tests
grep -c 'PASS\|FAIL\|SKIP' ./.newton/state/test_output.json

# Show failed tests
grep -A 2 -B 2 'FAIL' ./.newton/state/test_output.json
```
