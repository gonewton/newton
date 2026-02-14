# Newton Test Results Report
Generated: 2026-02-14T22:41:14Z
Command: ./scripts/run-tests.sh
Output File: test_results.md
JSON File: test_results.json

## Overall Status
✅ **PASSED** - All tests completed successfully

## Test Statistics
- **Total Tests:** 221
- **Passed:** 221
- **Failed:** 
- **Skipped:** 0
- **Passing Rate:** 100%

## Progress Visualization
```
[██████████████████████████████] 100% (221/221)
```

## Performance
- **Test Duration:** 0.105s

## Files
- **Raw Test Output:** `test_results.json`
- **Markdown Report:** `test_results.md`

## Raw Test Output
Complete test output is saved in: `test_results.json`

You can analyze it with standard Unix tools:
```bash
# Count total tests
grep -c 'PASS\|FAIL\|SKIP' test_results.json

# Show failed tests
grep -A 2 -B 2 'FAIL' test_results.json
```
