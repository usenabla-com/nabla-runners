#!/bin/bash
#
# Test script for tiltbridge repository build
# Fetches the latest commit from tiltbridge repo and sends it to nabla-runner
#

set -e

# Configuration
GITHUB_REPO="thorrak/tiltbridge"
RUNNER_URL="http://34.29.111.192:8080"
UPLOAD_URL="https://webhook.site/unique-id-here"  # Replace with actual upload endpoint

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

get_latest_commit_sha() {
    local repo=$1
    echo "Fetching latest commit..."
    
    local url="https://api.github.com/repos/${repo}/commits/main"
    local response
    local curl_exit_code
    
    echo "Requesting: $url"
    echo "DEBUG: About to run curl command"
    
    response=$(curl -v -s \
        --fail \
        --max-time 10 \
        --connect-timeout 5 \
        -H "Accept: application/vnd.github.v3+json" \
        -H "User-Agent: nabla-runner-test/1.0" \
        "$url" 2>&1)
    curl_exit_code=$?
    
    echo "DEBUG: curl finished with exit code: $curl_exit_code"
    
    if [ $curl_exit_code -ne 0 ]; then
        echo -e "${RED}❌ Error fetching commit info (curl exit code: $curl_exit_code)${NC}"
        echo "This might be due to network issues or GitHub API limits"
        return 1
    fi
    
    if [ -z "$response" ]; then
        echo -e "${RED}❌ Empty response from GitHub API${NC}"
        return 1
    fi
    
    local commit_sha
    commit_sha=$(echo "$response" | jq -r '.sha' 2>/dev/null)
    
    if [ $? -ne 0 ]; then
        echo -e "${RED}❌ Failed to parse JSON response${NC}"
        echo "Response was: $response"
        return 1
    fi
    
    if [ "$commit_sha" = "null" ] || [ -z "$commit_sha" ]; then
        echo -e "${RED}❌ Failed to parse commit SHA${NC}"
        echo "Response was: $response"
        return 1
    fi
    
    echo "$commit_sha"
}

build_archive_url() {
    local repo=$1
    local commit_sha=$2
    echo "https://github.com/${repo}/archive/${commit_sha}.tar.gz"
}

test_runner_health() {
    echo "Testing runner health at: ${RUNNER_URL}/health"
    
    local response
    local status_code
    
    response=$(curl -s -w "%{http_code}" \
        -H "User-Agent: nabla-runner-test/1.0" \
        --connect-timeout 10 \
        "${RUNNER_URL}/health")
    
    status_code="${response: -3}"
    response_body="${response%???}"
    
    if [ "$status_code" = "200" ]; then
        echo -e "${GREEN}✅ Runner is healthy:${NC}"
        echo "$response_body" | jq '.' 2>/dev/null || echo "$response_body"
        return 0
    else
        echo -e "${RED}❌ Health check failed: ${status_code}${NC}"
        return 1
    fi
}

test_tiltbridge_build() {
    echo "Testing build for repository: $GITHUB_REPO"
    
    # Get latest commit from GitHub API
    echo "Fetching latest commit..."
    local url="https://api.github.com/repos/${GITHUB_REPO}/commits/master"
    echo "Requesting: $url"
    
    local response
    response=$(curl -s --fail --max-time 10 --connect-timeout 5 \
        -H "Accept: application/vnd.github.v3+json" \
        -H "User-Agent: nabla-runner-test/1.0" \
        "$url" 2>/dev/null)
    local curl_exit_code=$?
    
    if [ $curl_exit_code -ne 0 ]; then
        echo -e "${RED}❌ Error fetching commit info (curl exit code: $curl_exit_code)${NC}"
        echo "Falling back to 'main' as commit reference"
        local commit_sha="main"
    else
        local commit_sha
        commit_sha=$(echo "$response" | jq -r '.sha' 2>/dev/null)
        
        if [ -z "$commit_sha" ] || [ "$commit_sha" = "null" ]; then
            echo -e "${YELLOW}⚠️ Could not parse commit SHA, using 'main'${NC}"
            local commit_sha="main"
        fi
    fi
    
    echo "Latest commit SHA: $commit_sha"
    
    # Build archive URL
    local archive_url
    archive_url=$(build_archive_url "$GITHUB_REPO" "$commit_sha")
    echo "Archive URL: $archive_url"
    
    # Extract owner and repo
    local owner repo_name
    IFS='/' read -r owner repo_name <<< "$GITHUB_REPO"
    
    # Build request URL with parameters
    local build_url="${RUNNER_URL}/build"
    local params="archive_url=$(printf '%s' "$archive_url" | jq -sRr @uri)"
    params+="&owner=$(printf '%s' "$owner" | jq -sRr @uri)"
    params+="&repo=$(printf '%s' "$repo_name" | jq -sRr @uri)"
    params+="&installation_id=12345"
    params+="&upload_url=$(printf '%s' "$UPLOAD_URL" | jq -sRr @uri)"
    
    echo ""
    echo "Sending build request to: $build_url"
    echo "Parameters:"
    echo "  archive_url: $archive_url"
    echo "  owner: $owner"
    echo "  repo: $repo_name" 
    echo "  installation_id: 12345"
    echo "  upload_url: $UPLOAD_URL"
    
    # Send build request
    local response
    local status_code
    
    response=$(curl -s -w "%{http_code}" \
        -X POST \
        -H "Content-Type: application/json" \
        -H "User-Agent: nabla-runner-test/1.0" \
        --connect-timeout 60 \
        "${build_url}?${params}")
    
    status_code="${response: -3}"
    response_body="${response%???}"
    
    echo ""
    echo "Response status: $status_code"
    
    # Try to parse as JSON, fall back to raw text
    if echo "$response_body" | jq empty 2>/dev/null; then
        echo "Response body:"
        echo "$response_body" | jq '.'
    else
        echo "Response body: $response_body"
    fi
    
    case "$status_code" in
        202)
            echo -e "\n${GREEN}✅ Build request accepted successfully!${NC}"
            ;;
        400)
            echo -e "\n${RED}❌ Bad request - check parameters${NC}"
            ;;
        500)
            echo -e "\n${RED}❌ Server error - build failed${NC}"
            ;;
        *)
            echo -e "\n${YELLOW}❓ Unexpected status code: ${status_code}${NC}"
            ;;
    esac
}

main() {
    echo "=== Nabla Runner Tiltbridge Test ==="
    echo ""
    
    # Check dependencies
    if ! command -v curl &> /dev/null; then
        echo -e "${RED}❌ curl is required but not installed${NC}"
        exit 1
    fi
    
    if ! command -v jq &> /dev/null; then
        echo -e "${RED}❌ jq is required but not installed${NC}"
        exit 1
    fi
    
    # First test runner health
    if ! test_runner_health; then
        echo -e "\n${RED}❌ Runner is not healthy, aborting test${NC}"
        exit 1
    fi
    
    echo ""
    echo "=================================================="
    echo ""
    
    # Run the build test
    test_tiltbridge_build
}

main "$@"