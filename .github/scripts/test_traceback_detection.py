#!/usr/bin/env python3
"""
Test script for traceback duplicate detection functionality.
Tests traceback extraction, normalization, and similarity calculation.
"""

import sys
import os

# Add the scripts directory to path to import the check_duplicate_issues module
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from check_duplicate_issues import (
    extract_traceback,
    normalize_traceback,
    calculate_traceback_similarity,
    combined_similarity_score
)


def print_header(text):
    """Print a formatted header."""
    print("\n" + "=" * 80)
    print(f"  {text}")
    print("=" * 80)


def print_test_result(test_name, passed, details=""):
    """Print test result."""
    status = "✓ PASS" if passed else "✗ FAIL"
    print(f"{status}: {test_name}")
    if details:
        print(f"  {details}")


def test_extract_traceback():
    """Test traceback extraction from various formats."""
    print_header("Testing Traceback Extraction")
    
    tests = [
        {
            "name": "Traceback in code block",
            "input": """
```python
Traceback (most recent call last):
  File "test.py", line 10, in main
    x = 1 / 0
ZeroDivisionError: division by zero
```
            """,
            "should_extract": True
        },
        {
            "name": "Traceback in plain text",
            "input": """
Error occurred:
Traceback (most recent call last):
  File "app/main.py", line 25, in process_data
    result = data[key]
KeyError: 'missing_key'
            """,
            "should_extract": True
        },
        {
            "name": "Traceback with user path",
            "input": """
```python
Traceback (most recent call last):
  File "C:\\Users\\john\\project\\app.py", line 42, in <module>
    process_file()
  File "C:\\Users\\john\\project\\utils.py", line 15, in process_file
    data = json.load(f)
FileNotFoundError: [Errno 2] No such file or directory: 'data.json'
```
            """,
            "should_extract": True
        },
        {
            "name": "No traceback",
            "input": "This is just regular text without any traceback.",
            "should_extract": False
        },
        {
            "name": "Traceback in template section",
            "input": """
**Traceback:**
```
Traceback (most recent call last):
  File "main.py", line 1, in <module>
    import nonexistent_module
ImportError: No module named 'nonexistent_module'
```
            """,
            "should_extract": True
        }
    ]
    
    passed = 0
    failed = 0
    
    for test in tests:
        result = extract_traceback(test["input"])
        extracted = result is not None
        should_extract = test["should_extract"]
        
        if extracted == should_extract:
            print_test_result(test["name"], True)
            if extracted:
                print(f"  Extracted: {result[:100]}...")
            passed += 1
        else:
            print_test_result(test["name"], False, 
                            f"Expected extraction={should_extract}, got {extracted}")
            failed += 1
    
    print(f"\nExtraction tests: {passed} passed, {failed} failed")
    return failed == 0


def test_normalize_traceback():
    """Test traceback normalization."""
    print_header("Testing Traceback Normalization")
    
    # Test case 1: Similar tracebacks with different paths should normalize similarly
    tb1 = """Traceback (most recent call last):
  File "C:\\Users\\alice\\project\\app.py", line 25, in process
    x = data['key']
KeyError: 'key'"""
    
    tb2 = """Traceback (most recent call last):
  File "/home/bob/project/app.py", line 30, in process
    x = data['key']
KeyError: 'key'"""
    
    # Test case 2: Tracebacks with different line numbers but same structure
    tb3 = """Traceback (most recent call last):
  File "utils.py", line 100, in function_a
    return function_b()
  File "utils.py", line 200, in function_b
    raise ValueError("Invalid input")
ValueError: Invalid input"""
    
    tb4 = """Traceback (most recent call last):
  File "utils.py", line 105, in function_a
    return function_b()
  File "utils.py", line 210, in function_b
    raise ValueError("Invalid input")
ValueError: Invalid input"""
    
    # Test case 3: Different errors should normalize differently
    tb5 = """Traceback (most recent call last):
  File "test.py", line 1, in <module>
    x = undefined_var
NameError: name 'undefined_var' is not defined"""
    
    normalized1 = normalize_traceback(tb1)
    normalized2 = normalize_traceback(tb2)
    normalized3 = normalize_traceback(tb3)
    normalized4 = normalize_traceback(tb4)
    normalized5 = normalize_traceback(tb5)
    
    print("Normalized traceback 1 (Windows path):")
    print(normalized1)
    print("\nNormalized traceback 2 (Linux path):")
    print(normalized2)
    print("\n✓ Both should have 'line N' instead of actual line numbers")
    print("✓ Both should have just 'app.py' not full paths")
    
    # Check if paths are normalized (should contain just filename)
    test1_passed = '"app.py"' in normalized1 and '"app.py"' in normalized2
    test2_passed = 'line N' in normalized1 and 'line N' in normalized2
    
    print_test_result("Path normalization (different user paths)", test1_passed)
    print_test_result("Line number normalization", test2_passed)
    
    print("\nNormalized traceback 3:")
    print(normalized3)
    print("\nNormalized traceback 4:")
    print(normalized4)
    print("\n✓ Should be similar (same structure, different line numbers)")
    
    # Tracebacks with same structure should be similar even with different line numbers
    from check_duplicate_issues import calculate_similarity
    sim_3_4 = calculate_similarity(normalized3, normalized4)
    test3_passed = sim_3_4 > 80.0
    
    print_test_result("Similar structure with different line numbers", test3_passed,
                     f"Similarity: {sim_3_4:.2f}%")
    
    # Different errors should be different
    # Note: We use calculate_traceback_similarity instead of raw similarity
    # because the error type penalty is applied there, which is how it's used in practice
    from check_duplicate_issues import calculate_traceback_similarity
    
    # Create mock issue bodies with the tracebacks
    body1 = f"Error occurred:\n```\n{tb1}\n```"
    body5 = f"Error occurred:\n```\n{tb5}\n```"
    
    sim_1_5 = calculate_traceback_similarity(body1, body5)
    test4_passed = sim_1_5 < 50.0
    
    print_test_result("Different errors stay different (with error type penalty)", test4_passed,
                     f"Similarity: {sim_1_5:.2f}% (KeyError vs NameError)")
    
    return test1_passed and test2_passed and test3_passed and test4_passed


def test_traceback_similarity():
    """Test traceback similarity calculation."""
    print_header("Testing Traceback Similarity Calculation")
    
    # Test case 1: Same error, different paths and line numbers
    issue1_body = """
**Error Details:**
I got this error when trying to process a file:

```
Traceback (most recent call last):
  File "C:\\Users\\john\\myproject\\main.py", line 42, in <module>
    process_data()
  File "C:\\Users\\john\\myproject\\utils.py", line 15, in process_data
    result = data['key']
KeyError: 'key'
```
    """
    
    issue2_body = """
**Error Details:**
Same error happened:

```python
Traceback (most recent call last):
  File "/home/user/project/main.py", line 50, in <module>
    process_data()
  File "/home/user/project/utils.py", line 18, in process_data
    result = data['key']
KeyError: 'key'
```
    """
    
    # Test case 2: Different errors
    issue3_body = """
```
Traceback (most recent call last):
  File "test.py", line 1, in <module>
    import missing_module
ImportError: No module named 'missing_module'
```
    """
    
    # Test case 3: One has traceback, other doesn't
    issue4_body = """
I got an error but forgot to include the traceback.
It was a KeyError.
    """
    
    sim_1_2 = calculate_traceback_similarity(issue1_body, issue2_body)
    sim_1_3 = calculate_traceback_similarity(issue1_body, issue3_body)
    sim_1_4 = calculate_traceback_similarity(issue1_body, issue4_body)
    
    print(f"Similarity between same error (different paths): {sim_1_2:.2f}%")
    print(f"Similarity between different errors: {sim_1_3:.2f}%")
    print(f"Similarity (one with traceback, one without): {sim_1_4:.2f}%")
    
    test1_passed = sim_1_2 > 70.0  # Should be high
    test2_passed = sim_1_3 < 50.0  # Should be low
    test3_passed = sim_1_4 < 40.0  # Should be low (only one has traceback)
    
    print_test_result("Same error with different paths/line numbers", test1_passed,
                     f"Score: {sim_1_2:.2f}%")
    print_test_result("Different errors are distinguished", test2_passed,
                     f"Score: {sim_1_3:.2f}%")
    print_test_result("Missing traceback handled correctly", test3_passed,
                     f"Score: {sim_1_4:.2f}%")
    
    return test1_passed and test2_passed and test3_passed


def test_combined_similarity():
    """Test combined similarity score with tracebacks."""
    print_header("Testing Combined Similarity Score")
    
    # Create two similar issues with tracebacks
    issue1 = {
        "title": "KeyError when processing data",
        "body": """
**Description:**
I'm getting a KeyError when trying to process some data.

**Traceback:**
```
Traceback (most recent call last):
  File "C:\\Users\\alice\\app\\main.py", line 25, in <module>
    process_data()
  File "C:\\Users\\alice\\app\\utils.py", line 10, in process_data
    value = data['key']
KeyError: 'key'
```

**Steps to reproduce:**
1. Run the script
2. Error occurs
        """
    }
    
    issue2 = {
        "title": "KeyError error in data processing",
        "body": """
I encountered the same KeyError issue.

```python
Traceback (most recent call last):
  File "/home/bob/project/main.py", line 30, in <module>
    process_data()
  File "/home/bob/project/utils.py", line 12, in process_data
    value = data['key']
KeyError: 'key'
```
        """
    }
    
    # Different issue
    issue3 = {
        "title": "ImportError with module",
        "body": """
```
Traceback (most recent call last):
  File "test.py", line 1, in <module>
    import missing
ImportError: No module named 'missing'
```
        """
    }
    
    score1_2, details1_2 = combined_similarity_score(issue1, issue2)
    score1_3, details1_3 = combined_similarity_score(issue1, issue3)
    
    print(f"Issue 1 vs Issue 2 (same error, different paths):")
    print(f"  Combined score: {score1_2:.2f}%")
    print(f"  Title similarity: {details1_2['title_similarity']:.2f}%")
    print(f"  Body similarity: {details1_2['body_similarity']:.2f}%")
    print(f"  Traceback similarity: {details1_2['traceback_similarity']:.2f}%")
    print(f"  Keyword overlap: {details1_2['keyword_overlap']:.2f}%")
    
    print(f"\nIssue 1 vs Issue 3 (different errors):")
    print(f"  Combined score: {score1_3:.2f}%")
    print(f"  Traceback similarity: {details1_3['traceback_similarity']:.2f}%")
    
    test1_passed = score1_2 > 60.0  # Should be identified as potential duplicate
    test2_passed = details1_2['traceback_similarity'] > 70.0  # Tracebacks should match
    test3_passed = score1_3 < 50.0  # Different issues should have lower score
    
    print_test_result("Similar issues with tracebacks identified", test1_passed,
                     f"Score: {score1_2:.2f}%")
    print_test_result("Traceback similarity calculated correctly", test2_passed,
                     f"Traceback sim: {details1_2['traceback_similarity']:.2f}%")
    print_test_result("Different issues stay distinct", test3_passed,
                     f"Score: {score1_3:.2f}%")
    
    return test1_passed and test2_passed and test3_passed


def test_real_world_scenarios():
    """Test with more realistic scenarios."""
    print_header("Testing Real-World Scenarios")
    
    scenarios = [
        {
            "name": "Same error, Windows vs Linux paths",
            "issue1": {
                "title": "FileNotFoundError on Windows",
                "body": """
```
Traceback (most recent call last):
  File "C:\\Users\\john\\project\\main.py", line 10, in <module>
    with open('data.txt', 'r') as f:
FileNotFoundError: [Errno 2] No such file or directory: 'data.txt'
```
                """
            },
            "issue2": {
                "title": "FileNotFoundError on Linux",
                "body": """
```
Traceback (most recent call last):
  File "/home/user/project/main.py", line 15, in <module>
    with open('data.txt', 'r') as f:
FileNotFoundError: [Errno 2] No such file or directory: 'data.txt'
```
                """
            },
            "should_match": True
        },
        {
            "name": "Different error types",
            "issue1": {
                "title": "TypeError occurred",
                "body": """
```
Traceback (most recent call last):
  File "app.py", line 5, in <module>
    result = "string" + 42
TypeError: can only concatenate str (not "int") to str
```
                """
            },
            "issue2": {
                "title": "ValueError occurred",
                "body": """
```
Traceback (most recent call last):
  File "app.py", line 5, in <module>
    int("not a number")
ValueError: invalid literal for int() with base 10: 'not a number'
```
                """
            },
            "should_match": False
        },
        {
            "name": "Same error, different line numbers",
            "issue1": {
                "title": "AttributeError",
                "body": """
```
Traceback (most recent call last):
  File "utils.py", line 100, in <module>
    obj.nonexistent()
AttributeError: 'MyClass' object has no attribute 'nonexistent'
```
                """
            },
            "issue2": {
                "title": "AttributeError",
                "body": """
```
Traceback (most recent call last):
  File "utils.py", line 105, in <module>
    obj.nonexistent()
AttributeError: 'MyClass' object has no attribute 'nonexistent'
```
                """
            },
            "should_match": True
        }
    ]
    
    passed = 0
    failed = 0
    
    for scenario in scenarios:
        score, details = combined_similarity_score(scenario["issue1"], scenario["issue2"])
        traceback_sim = details.get('traceback_similarity', 0.0)
        
        if scenario["should_match"]:
            # Should have high similarity (either overall or traceback)
            matched = score >= 60.0 or traceback_sim >= 70.0
        else:
            # Should have low similarity
            matched = score < 50.0 and traceback_sim < 50.0
        
        if matched == scenario["should_match"]:
            status = "match" if scenario["should_match"] else "not match"
            print_test_result(scenario["name"], True,
                            f"Score: {score:.2f}%, Traceback: {traceback_sim:.2f}% (should {status})")
            passed += 1
        else:
            print_test_result(scenario["name"], False,
                            f"Score: {score:.2f}%, Traceback: {traceback_sim:.2f}% (expected should_match={scenario['should_match']})")
            failed += 1
    
    print(f"\nReal-world tests: {passed} passed, {failed} failed")
    return failed == 0


def main():
    """Run all tests."""
    print("\n" + "=" * 80)
    print("  TRACEBACK DUPLICATE DETECTION TEST SUITE")
    print("=" * 80)
    
    results = []
    
    results.append(("Traceback Extraction", test_extract_traceback()))
    results.append(("Traceback Normalization", test_normalize_traceback()))
    results.append(("Traceback Similarity", test_traceback_similarity()))
    results.append(("Combined Similarity", test_combined_similarity()))
    results.append(("Real-World Scenarios", test_real_world_scenarios()))
    
    print_header("Test Summary")
    all_passed = True
    for test_name, passed in results:
        status = "✓ PASS" if passed else "✗ FAIL"
        print(f"{status}: {test_name}")
        if not passed:
            all_passed = False
    
    print("\n" + "=" * 80)
    if all_passed:
        print("  ALL TESTS PASSED ✓")
    else:
        print("  SOME TESTS FAILED ✗")
    print("=" * 80)
    
    return 0 if all_passed else 1


if __name__ == '__main__':
    sys.exit(main())

