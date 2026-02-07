# Auto-Release Workflow Improvements

## Changes Made

### 1. **Branch Conflict Prevention**
- **Problem:** Workflow failed when `auto-release-X.Y.Z` branch already existed with different history
- **Solution:** Delete remote and local branches if they exist before creating new ones
- **Code:** Added cleanup logic in "Create release branch" step

### 2. **Safe Force Pushing**
- **Changed:** From `git push` to `git push --force-with-lease`
- **Benefit:** Prevents accidental overwrites while allowing controlled force pushes
- **Safety:** `--force-with-lease` only force-pushes if remote hasn't changed since last fetch

### 3. **Duplicate PR Prevention**
- **Problem:** Could create multiple PRs for the same release
- **Solution:** Check if PR exists and update it instead of creating a duplicate
- **Code:** Added PR existence check using `gh pr list`

### 4. **Automatic Cleanup Job**
- **New Job:** `cleanup-old-releases`
- **Purpose:** Removes stale release branches automatically
- **Criteria:**
  - Deletes branches whose PRs are merged
  - Deletes branches older than 7 days with no PR
- **Benefit:** Prevents branch accumulation over time

### 5. **Better Error Handling**
- Added `|| true` fallbacks for delete operations
- Added informative echo messages for debugging
- Graceful handling of non-existent branches

## Best Practices Implemented

1. ✅ **Idempotent Operations** - Can be run multiple times safely
2. ✅ **Cleanup Automation** - No manual branch management needed
3. ✅ **Force Push Safety** - Uses `--force-with-lease` instead of `--force`
4. ✅ **Duplicate Prevention** - Checks before creating resources
5. ✅ **Comprehensive Logging** - Clear messages for debugging
6. ✅ **Error Resilience** - Continues even if cleanup fails

## Testing Recommendations

1. **Trigger the workflow** by pushing to main
2. **Verify branch creation** succeeds
3. **Re-trigger workflow** to verify branch cleanup works
4. **Check PR updates** instead of duplicates
5. **Wait 7+ days** to verify old branch cleanup (or manually test)

## Resolves

- ❌ "non-fast-forward" git push errors
- ❌ Stale release branch accumulation
- ❌ Duplicate PR creation
- ❌ Manual branch cleanup requirements
