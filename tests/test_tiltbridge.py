#!/usr/bin/env python3
"""
Test script for tiltbridge repository build
Fetches the latest commit from tiltbridge repo and sends it to nabla-runner
"""

import requests
import json
import sys
from urllib.parse import quote

# Configuration
GITHUB_REPO = "thorrak/tiltbridge"
RUNNER_URL = "http://34.29.111.192:8080"
UPLOAD_URL = "https://webhook.site/unique-id-here"  # Replace with actual upload endpoint

def get_latest_commit_sha(repo):
    """Get the latest commit SHA from GitHub API"""
    try:
        url = f"https://api.github.com/repos/{repo}/commits/main"
        response = requests.get(url, headers={
            "Accept": "application/vnd.github.v3+json",
            "User-Agent": "nabla-runner-test/1.0"
        })
        response.raise_for_status()
        
        commit_data = response.json()
        return commit_data["sha"]
    
    except requests.RequestException as e:
        print(f"Error fetching commit info: {e}")
        return None

def build_archive_url(repo, commit_sha):
    """Build the archive URL for the given commit"""
    return f"https://github.com/{repo}/archive/{commit_sha}.tar.gz"

def test_tiltbridge_build():
    """Main test function"""
    print(f"Testing build for repository: {GITHUB_REPO}")
    
    # Get latest commit
    print("Fetching latest commit...")
    commit_sha = get_latest_commit_sha(GITHUB_REPO)
    if not commit_sha:
        print("Failed to get latest commit SHA")
        sys.exit(1)
    
    print(f"Latest commit SHA: {commit_sha}")
    
    # Build archive URL
    archive_url = build_archive_url(GITHUB_REPO, commit_sha)
    print(f"Archive URL: {archive_url}")
    
    # Prepare build request
    owner, repo = GITHUB_REPO.split("/")
    build_params = {
        "archive_url": archive_url,
        "owner": owner,
        "repo": repo,
        "installation_id": "12345",  # Mock installation ID
        "upload_url": UPLOAD_URL
    }
    
    # Build request URL
    build_url = f"{RUNNER_URL}/build"
    
    print(f"\nSending build request to: {build_url}")
    print(f"Parameters: {json.dumps(build_params, indent=2)}")
    
    try:
        # Send build request
        response = requests.post(
            build_url,
            params=build_params,
            headers={
                "Content-Type": "application/json",
                "User-Agent": "nabla-runner-test/1.0"
            },
            timeout=60
        )
        
        print(f"\nResponse status: {response.status_code}")
        print(f"Response headers: {dict(response.headers)}")
        
        if response.headers.get('content-type', '').startswith('application/json'):
            try:
                response_data = response.json()
                print(f"Response body: {json.dumps(response_data, indent=2)}")
            except json.JSONDecodeError:
                print(f"Response body (raw): {response.text}")
        else:
            print(f"Response body: {response.text}")
        
        if response.status_code == 202:
            print("\n✅ Build request accepted successfully!")
        elif response.status_code == 400:
            print("\n❌ Bad request - check parameters")
        elif response.status_code == 500:
            print("\n❌ Server error - build failed")
        else:
            print(f"\n❓ Unexpected status code: {response.status_code}")
            
    except requests.RequestException as e:
        print(f"\n❌ Request failed: {e}")
        sys.exit(1)

def test_runner_health():
    """Test if the runner service is healthy"""
    try:
        health_url = f"{RUNNER_URL}/health"
        print(f"Testing runner health at: {health_url}")
        
        response = requests.get(health_url, timeout=10)
        
        if response.status_code == 200:
            health_data = response.json()
            print(f"✅ Runner is healthy: {json.dumps(health_data, indent=2)}")
            return True
        else:
            print(f"❌ Health check failed: {response.status_code}")
            return False
            
    except requests.RequestException as e:
        print(f"❌ Health check failed: {e}")
        return False

if __name__ == "__main__":
    print("=== Nabla Runner Tiltbridge Test ===\n")
    
    # First test runner health
    if not test_runner_health():
        print("\n❌ Runner is not healthy, aborting test")
        sys.exit(1)
    
    print("\n" + "="*50 + "\n")
    
    # Run the build test
    test_tiltbridge_build()