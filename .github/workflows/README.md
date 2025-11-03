# GitHub Workflows

This directory contains GitHub Actions workflows for the EasyScanlate project.

## Check Duplicate Issues Workflow

**File:** `.github/workflows/check-duplicate-issues.yml`

### Overview

This workflow automatically detects potential duplicate issues when a new issue is opened. It compares the new issue's title and body with all existing open issues using text similarity algorithms.

### How It Works

1. **Trigger**: Runs automatically when a new issue is opened
2. **Comparison**: Uses multiple similarity metrics:
   - Title similarity (50% weight)
   - Body similarity (30% weight)
   - Keyword overlap (20% weight)
3. **Threshold**: Issues with ≥60% similarity are flagged as potential duplicates
4. **Action**: If duplicates are found:
   - Adds a comment listing similar issues with similarity scores
   - Adds a `potential-duplicate` label to the issue
   - The label is automatically created if it doesn't exist

### Features

- **Intelligent Matching**: Uses Levenshtein distance algorithm for accurate text comparison
- **Fallback Method**: Falls back to word-based Jaccard similarity if the Levenshtein library isn't available
- **Issue Template Support**: Automatically extracts meaningful content from GitHub issue templates, ignoring boilerplate and placeholder text
- **Section-Based Comparison**: Compares individual template sections (Description, Steps to Reproduce, Expected/Actual Behavior, etc.) separately for better accuracy
- **Placeholder Detection**: Filters out template placeholders like "[Please replace this line...]" to focus on actual user content
- **Multiple Similar Issues**: Can detect multiple similar issues and list them all
- **Similarity Scores**: Shows percentage similarity for each potential duplicate
- **Top 5 Matches**: Limits results to the top 5 most similar issues

### Requirements

The workflow uses Python and requires the following packages:
- `requests` - For GitHub API calls
- `python-levenshtein` - For text similarity calculations (optional, has fallback)

These are automatically installed by the workflow.

### Permissions

The workflow requires:
- `issues: write` - To comment and add labels to issues
- `contents: read` - To read repository contents

Note: Creating new labels may require repository write access. The workflow will attempt to create the label automatically, and if it fails due to permissions, it will still add the comment.

### Customization

You can customize the workflow by editing `.github/scripts/check_duplicate_issues.py`:

- **Similarity Threshold**: Change `similarity_threshold` (default: 60.0) to be more or less strict
- **Weight Distribution**: Adjust the weights in `combined_similarity_score()` function:
  - Title weight (currently 0.5)
  - Body weight (currently 0.3)
  - Keyword overlap weight (currently 0.2)
- **Maximum Results**: Change the limit in `duplicates[:5]` to show more or fewer matches

### Manual Setup

The `potential-duplicate` label will be created automatically if it doesn't exist. However, you can also create it manually in your repository settings:

1. Go to Issues → Labels in your GitHub repository
2. Create a new label named `potential-duplicate`
3. Choose a color (suggested: `#d4c5f9`)
4. Add description: "This issue may be a duplicate of another open issue"

### Template Handling

The workflow intelligently handles GitHub issue templates by:

1. **Extracting Template Sections**: Automatically identifies and extracts content from common template sections:
   - Description
   - Error Details
   - Traceback/Full Traceback
   - Steps to Reproduce
   - Expected Behavior
   - Actual Behavior
   - System Information
   - Additional Information

2. **Filtering Placeholders**: Removes placeholder text like:
   - `[Please replace this line...]`
   - `(Please describe...)`
   - Empty sections or sections with only instructions

3. **Meaningful Content Focus**: Compares the actual user-provided content rather than template boilerplate, leading to more accurate duplicate detection.

4. **Section-Based Matching**: When both issues use templates, compares matching sections (e.g., "Steps to Reproduce" vs "Steps to Reproduce") for higher accuracy.

### Notes

- The workflow only compares against **open** issues (closed issues are ignored)
- It excludes the current issue from comparison
- Pull requests are automatically excluded from comparison
- The workflow respects rate limits and paginates through issues properly
- Template extraction works with both markdown headers (`## Section`) and bold text (`**Section**`)

