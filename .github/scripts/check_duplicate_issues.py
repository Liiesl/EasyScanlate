#!/usr/bin/env python3
"""
Script to check for duplicate issues by comparing new issues with existing open issues.
Uses text similarity algorithms to detect potential duplicates.
"""

import os
import sys
import re
import requests
from typing import List, Dict, Tuple, Optional


def normalize_text(text: str) -> str:
    """Normalize text for comparison by converting to lowercase and removing extra whitespace."""
    if not text:
        return ""
    return " ".join(text.lower().split())


def is_placeholder_text(text: str) -> bool:
    """Check if text appears to be placeholder/instruction text from templates."""
    if not text:
        return False
    
    text_lower = text.lower().strip()
    
    # Common placeholder patterns
    placeholder_patterns = [
        r'^\s*\[.*please.*\]',
        r'^\s*\(.*please.*\)',
        r'please replace',
        r'please describe',
        r'please add',
        r'please provide',
        r'what you expected',
        r'what actually happened',
        r'steps to reproduce',
        r'describe what',
        r'^n/a\s*$',
        r'^none\s*$',
        r'^\s*-\s*$',  # Just a dash
        r'^\s*$',  # Empty
    ]
    
    for pattern in placeholder_patterns:
        if re.search(pattern, text_lower, re.IGNORECASE):
            return True
    
    # Check if text is very short and looks like placeholder
    if len(text_lower) < 20 and any(word in text_lower for word in ['replace', 'describe', 'add']):
        return True
    
    return False


def extract_template_sections(body: str) -> Dict[str, str]:
    """
    Extract content from common GitHub issue template sections.
    Returns a dictionary mapping section names to their content.
    """
    if not body:
        return {}
    
    sections = {}
    
    # Common section headers (both markdown and bold formats)
    section_patterns = {
        'description': [r'#+\s*description\s*:?\s*\n', r'\*\*description\s*:?\*\*\s*\n', r'description\s*:?\s*\n'],
        'error_details': [r'#+\s*error\s+details\s*:?\s*\n', r'\*\*error\s+details\s*:?\*\*\s*\n'],
        'traceback': [r'#+\s*(?:full\s+)?traceback\s*:?\s*\n', r'\*\*(?:full\s+)?traceback\s*:?\*\*\s*\n'],
        'steps_to_reproduce': [r'#+\s*steps\s+to\s+reproduce\s*:?\s*\n', r'\*\*steps\s+to\s+reproduce\s*:?\*\*\s*\n'],
        'expected_behavior': [r'#+\s*expected\s+behavior\s*:?\s*\n', r'\*\*expected\s+behavior\s*:?\*\*\s*\n'],
        'actual_behavior': [r'#+\s*actual\s+behavior\s*:?\s*\n', r'\*\*actual\s+behavior\s*:?\*\*\s*\n'],
        'system_info': [r'#+\s*system\s+(?:information|info)\s*:?\s*\n', r'\*\*system\s+(?:information|info)\s*:?\*\*\s*\n'],
        'additional_info': [r'#+\s*additional\s+(?:information|info)\s*:?\s*\n', r'\*\*additional\s+(?:information|info)\s*:?\*\*\s*\n'],
    }
    
    # Try to extract each section
    for section_name, patterns in section_patterns.items():
        for pattern in patterns:
            # Look for section header
            match = re.search(pattern, body, re.IGNORECASE | re.MULTILINE)
            if match:
                start_pos = match.end()
                
                # Find the next section header or end of body
                next_header_pattern = r'\n\s*(?:#{1,6}\s+\*\*?|#+\s+|\*\*)\s*[a-z]'
                next_match = re.search(next_header_pattern, body[start_pos:], re.IGNORECASE | re.MULTILINE)
                
                if next_match:
                    end_pos = start_pos + next_match.start()
                    section_content = body[start_pos:end_pos].strip()
                else:
                    section_content = body[start_pos:].strip()
                
                # Clean up the content
                section_content = re.sub(r'```[^`]*```', '', section_content)  # Remove code blocks temporarily
                section_content = re.sub(r'`[^`]+`', '', section_content)  # Remove inline code
                section_content = re.sub(r'^[-*]\s*', '', section_content, flags=re.MULTILINE)  # Remove list markers
                section_content = section_content.strip()
                
                # Only keep non-placeholder content
                if section_content and not is_placeholder_text(section_content) and len(section_content) > 10:
                    # Keep the longest content if multiple matches
                    if section_name not in sections or len(section_content) > len(sections[section_name]):
                        sections[section_name] = section_content
                break
    
    return sections


def extract_meaningful_content(body: str) -> str:
    """
    Extract meaningful content from issue body by removing template boilerplate,
    placeholders, and common template sections headers.
    """
    if not body:
        return ""
    
    content = body
    
    # Remove code blocks (tracebacks are usually in code blocks, but we'll extract separately)
    # We'll keep them for now but extract them separately in extract_template_sections
    
    # Remove common template headers and instructions
    template_patterns = [
        r'---\s*\n',  # Horizontal rules
        r'#+\s*(?:description|error details|traceback|steps to reproduce|expected behavior|actual behavior|system information|additional information)\s*:?\s*\n',
        r'\*\*(?:description|error details|traceback|steps to reproduce|expected behavior|actual behavior|system information|additional information)\s*:?\*\*\s*\n',
        r'Please\s+(?:replace|describe|add|provide).*?\n',
        r'\[Please[^\]]+\]',
        r'\(Please[^\)]+\)',
    ]
    
    for pattern in template_patterns:
        content = re.sub(pattern, '\n', content, flags=re.IGNORECASE | re.MULTILINE)
    
    # Extract sections and combine meaningful content
    sections = extract_template_sections(body)
    meaningful_parts = []
    
    # Add meaningful sections
    for section_name, section_content in sections.items():
        if section_content and not is_placeholder_text(section_content):
            meaningful_parts.append(section_content)
    
    # Also check for any remaining meaningful text outside of sections
    # Remove all markdown headers and formatting
    remaining = re.sub(r'#{1,6}\s+', '', content)
    remaining = re.sub(r'\*\*([^\*]+)\*\*', r'\1', remaining)
    remaining = re.sub(r'```[^`]*```', ' ', remaining)  # Remove code blocks
    remaining = re.sub(r'`[^`]+`', ' ', remaining)  # Remove inline code
    remaining = re.sub(r'^\s*[-*]\s+', '', remaining, flags=re.MULTILINE)  # Remove list markers
    remaining = re.sub(r'^\s*\d+\.\s+', '', remaining, flags=re.MULTILINE)  # Remove numbered lists
    remaining = re.sub(r'\n+', ' ', remaining)  # Normalize whitespace
    remaining = remaining.strip()
    
    # Only include if it doesn't look like placeholder
    if remaining and not is_placeholder_text(remaining) and len(remaining) > 20:
        # Check if this content is already in one of the sections
        if not any(remaining.lower() in part.lower() or part.lower() in remaining.lower() for part in meaningful_parts):
            meaningful_parts.append(remaining)
    
    # Combine all meaningful parts
    combined = ' '.join(meaningful_parts)
    return combined.strip()


def calculate_similarity(text1: str, text2: str) -> float:
    """
    Calculate similarity between two texts using multiple methods.
    Returns a score between 0 and 100.
    """
    try:
        from Levenshtein import distance, ratio
    except ImportError:
        # Fallback to simple word-based comparison
        words1 = set(normalize_text(text1).split())
        words2 = set(normalize_text(text2).split())
        
        if not words1 or not words2:
            return 0.0
        
        intersection = words1.intersection(words2)
        union = words1.union(words2)
        
        if not union:
            return 0.0
        
        jaccard = len(intersection) / len(union)
        return jaccard * 100
    
    # Use Levenshtein distance for better accuracy
    text1_norm = normalize_text(text1)
    text2_norm = normalize_text(text2)
    
    if not text1_norm or not text2_norm:
        return 0.0
    
    # Calculate ratio (0-1) and convert to percentage
    similarity_ratio = ratio(text1_norm, text2_norm)
    return similarity_ratio * 100


def extract_keywords(text: str) -> set:
    """Extract important keywords from text (simple implementation)."""
    if not text:
        return set()
    
    # Common words to ignore
    stop_words = {
        'the', 'a', 'an', 'and', 'or', 'but', 'in', 'on', 'at', 'to', 'for',
        'of', 'with', 'by', 'is', 'are', 'was', 'were', 'be', 'been', 'have',
        'has', 'had', 'do', 'does', 'did', 'will', 'would', 'should', 'could',
        'this', 'that', 'these', 'those', 'i', 'you', 'he', 'she', 'it', 'we', 'they',
        'when', 'where', 'what', 'why', 'how', 'can', 'may', 'might', 'must'
    }
    
    text_lower = text.lower()
    # Split by non-alphanumeric characters
    words = [word.strip('.,!?;:()[]{}"\'-_') for word in text_lower.split() if word.strip('.,!?;:()[]{}"\'-_')]
    keywords = {word for word in words if len(word) > 2 and word not in stop_words}
    return keywords


def calculate_title_similarity(title1: str, title2: str) -> float:
    """Calculate similarity between two issue titles."""
    return calculate_similarity(title1, title2)


def calculate_body_similarity(body1: str, body2: str) -> float:
    """
    Calculate similarity between two issue bodies.
    Extracts meaningful content from templates before comparing.
    """
    # Extract meaningful content (excluding template boilerplate)
    meaningful1 = extract_meaningful_content(body1)
    meaningful2 = extract_meaningful_content(body2)
    
    # If we extracted meaningful content, use that; otherwise fall back to full body
    if meaningful1 and meaningful2:
        return calculate_similarity(meaningful1, meaningful2)
    elif meaningful1 or meaningful2:
        # One has meaningful content, the other might be empty - compare what we have
        return calculate_similarity(meaningful1 or body1, meaningful2 or body2)
    else:
        # Fall back to full body comparison
        return calculate_similarity(body1, body2)


def calculate_section_similarity(body1: str, body2: str) -> Dict[str, float]:
    """
    Calculate similarity for individual template sections.
    Returns a dictionary of section similarities.
    """
    sections1 = extract_template_sections(body1)
    sections2 = extract_template_sections(body2)
    
    section_similarities = {}
    
    # Compare matching sections
    all_section_names = set(sections1.keys()) | set(sections2.keys())
    
    for section_name in all_section_names:
        content1 = sections1.get(section_name, '')
        content2 = sections2.get(section_name, '')
        
        if content1 and content2:
            section_similarities[section_name] = calculate_similarity(content1, content2)
        elif content1 or content2:
            # One has content, the other doesn't - lower similarity
            section_similarities[section_name] = 0.0
    
    return section_similarities


def calculate_keyword_overlap(text1: str, text2: str) -> float:
    """Calculate keyword overlap between two texts."""
    keywords1 = extract_keywords(text1)
    keywords2 = extract_keywords(text2)
    
    if not keywords1 or not keywords2:
        return 0.0
    
    intersection = keywords1.intersection(keywords2)
    union = keywords1.union(keywords2)
    
    if not union:
        return 0.0
    
    return (len(intersection) / len(union)) * 100


def combined_similarity_score(new_issue: Dict, existing_issue: Dict) -> Tuple[float, Dict]:
    """
    Calculate a combined similarity score between two issues.
    Accounts for issue templates by extracting meaningful content.
    Returns (score, details) where score is 0-100 and details contains breakdown.
    """
    new_title = new_issue.get('title', '')
    new_body = new_issue.get('body', '')
    existing_title = existing_issue.get('title', '')
    existing_body = existing_issue.get('body', '')
    
    # Extract meaningful content from bodies
    new_meaningful = extract_meaningful_content(new_body)
    existing_meaningful = extract_meaningful_content(existing_body)
    
    # Calculate individual similarities
    title_sim = calculate_title_similarity(new_title, existing_title)
    
    # Body similarity using meaningful content
    if new_meaningful and existing_meaningful:
        body_sim = calculate_body_similarity(new_body, existing_body)
    else:
        # Fall back to full body if no meaningful content extracted
        body_sim = calculate_similarity(new_body, existing_body)
    
    # Section-based similarity (if both have template sections)
    section_sims = calculate_section_similarity(new_body, existing_body)
    section_avg = 0.0
    if section_sims:
        section_avg = sum(section_sims.values()) / len(section_sims)
        # Boost body similarity if sections match well
        body_sim = max(body_sim, section_avg * 0.8)
    
    # Keyword overlap using meaningful content
    meaningful_text1 = new_title + ' ' + (new_meaningful or new_body)
    meaningful_text2 = existing_title + ' ' + (existing_meaningful or existing_body or '')
    keyword_overlap = calculate_keyword_overlap(meaningful_text1, meaningful_text2)
    
    # Weighted combination (title is more important, meaningful body content gets more weight)
    combined_score = (title_sim * 0.45) + (body_sim * 0.35) + (keyword_overlap * 0.15) + (section_avg * 0.05)
    
    details = {
        'title_similarity': round(title_sim, 2),
        'body_similarity': round(body_sim, 2),
        'keyword_overlap': round(keyword_overlap, 2),
        'section_similarity': round(section_avg, 2) if section_sims else 0.0,
        'combined_score': round(combined_score, 2)
    }
    
    return combined_score, details


def fetch_open_issues(token: str, owner: str, repo: str, exclude_number: int) -> List[Dict]:
    """Fetch all open issues except the one being checked."""
    headers = {
        'Authorization': f'token {token}',
        'Accept': 'application/vnd.github.v3+json'
    }
    
    issues = []
    page = 1
    per_page = 100
    
    while True:
        url = f'https://api.github.com/repos/{owner}/{repo}/issues'
        params = {
            'state': 'open',
            'page': page,
            'per_page': per_page,
            'sort': 'created',
            'direction': 'desc'
        }
        
        response = requests.get(url, headers=headers, params=params)
        response.raise_for_status()
        
        page_issues = response.json()
        
        # Filter out pull requests and the current issue
        filtered = [
            issue for issue in page_issues
            if 'pull_request' not in issue and issue['number'] != exclude_number
        ]
        
        issues.extend(filtered)
        
        # Check if there are more pages
        if len(page_issues) < per_page:
            break
        
        page += 1
    
    return issues


def main():
    """Main function to check for duplicate issues."""
    github_token = os.environ.get('GITHUB_TOKEN')
    issue_number = int(os.environ.get('ISSUE_NUMBER', '0'))
    issue_title = os.environ.get('ISSUE_TITLE', '')
    issue_body = os.environ.get('ISSUE_BODY', '')
    
    # Get repository info from GitHub environment
    github_repository = os.environ.get('GITHUB_REPOSITORY', '')
    if not github_repository:
        print("Error: GITHUB_REPOSITORY not set")
        sys.exit(1)
    
    owner, repo = github_repository.split('/', 1)
    
    if not github_token:
        print("Error: GITHUB_TOKEN not set")
        sys.exit(1)
    
    if issue_number == 0:
        print("Error: ISSUE_NUMBER not set")
        sys.exit(1)
    
    # Create new issue dict
    new_issue = {
        'number': issue_number,
        'title': issue_title or '',
        'body': issue_body or ''
    }
    
    print(f"Checking for duplicates of issue #{issue_number}...")
    print(f"Title: {issue_title}")
    
    # Fetch all open issues
    try:
        open_issues = fetch_open_issues(github_token, owner, repo, issue_number)
        print(f"Found {len(open_issues)} open issues to compare against")
    except Exception as e:
        print(f"Error fetching issues: {e}")
        sys.exit(1)
    
    # Compare with existing issues
    duplicates = []
    similarity_threshold = 60.0  # Minimum similarity score to consider as duplicate
    
    for existing_issue in open_issues:
        score, details = combined_similarity_score(new_issue, existing_issue)
        
        if score >= similarity_threshold:
            duplicates.append({
                'number': existing_issue['number'],
                'title': existing_issue['title'],
                'url': existing_issue['html_url'],
                'score': score,
                'details': details
            })
            print(f"  Potential duplicate: #{existing_issue['number']} (similarity: {score:.2f}%)")
    
    # Sort by similarity score (highest first)
    duplicates.sort(key=lambda x: x['score'], reverse=True)
    
    # Limit to top 5 most similar
    duplicates = duplicates[:5]
    
    # Set outputs
    if duplicates:
        duplicate_numbers = ','.join(str(dup['number']) for dup in duplicates)
        similarity_scores = ','.join(f"{dup['score']:.1f}" for dup in duplicates)
        
        print(f"\n✓ Found {len(duplicates)} potential duplicate(s)")
        
        # Set GitHub Actions outputs using the new format
        github_output = os.environ.get('GITHUB_OUTPUT', '')
        if github_output:
            with open(github_output, 'a') as f:
                f.write(f"has_duplicates=true\n")
                f.write(f"duplicate_issue_numbers={duplicate_numbers}\n")
                f.write(f"similarity_scores={similarity_scores}\n")
    else:
        print("\n✓ No duplicates found")
        
        # Set GitHub Actions outputs using the new format
        github_output = os.environ.get('GITHUB_OUTPUT', '')
        if github_output:
            with open(github_output, 'a') as f:
                f.write(f"has_duplicates=false\n")
    
    return 0


if __name__ == '__main__':
    sys.exit(main())

