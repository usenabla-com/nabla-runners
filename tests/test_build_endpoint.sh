#!/bin/bash

# Test script for the nabla-runner /build endpoint
# This simulates the full flow of sending a repository ZIP to the build service

set -e

# Configuration
SERVER_URL="${SERVER_URL:-http://localhost:8080}"
OWNER="test-owner"
REPO="test-repo"
HEAD_SHA="abc123def456"
INSTALLATION_ID="12345"
UPLOAD_URL="http://example.com/upload"  # Mock upload URL

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${YELLOW}Testing Nabla Runner Build Endpoint${NC}"
echo "Server: $SERVER_URL"
echo ""

# Step 1: Check if server is running
echo -e "${YELLOW}Step 1: Checking server health...${NC}"
HEALTH_RESPONSE=$(curl -s "$SERVER_URL/health" || echo "")
if [[ $HEALTH_RESPONSE == *"healthy"* ]]; then
    echo -e "${GREEN}✓ Server is healthy${NC}"
    echo "Response: $HEALTH_RESPONSE"
else
    echo -e "${RED}✗ Server is not responding${NC}"
    echo "Please start the server with: cargo run --bin nabla-runner"
    exit 1
fi
echo ""

# Step 2: Create a test repository
echo -e "${YELLOW}Step 2: Creating test repository...${NC}"
TEST_DIR=$(mktemp -d)
cd "$TEST_DIR"

# Create a simple Rust project
mkdir -p test-firmware
cd test-firmware

# Create Cargo.toml
cat > Cargo.toml << 'EOF'
[package]
name = "test-firmware"
version = "0.1.0"
edition = "2021"

[dependencies]
EOF

# Create src/main.rs
mkdir -p src
cat > src/main.rs << 'EOF'
fn main() {
    println!("Test firmware build successful!");
    println!("Version: 0.1.0");
}
EOF

echo -e "${GREEN}✓ Created test Rust project${NC}"
cd ..
echo ""

# Step 3: Create ZIP file
echo -e "${YELLOW}Step 3: Creating ZIP archive...${NC}"
zip -r test-firmware.zip test-firmware/ > /dev/null 2>&1
ZIP_SIZE=$(ls -lh test-firmware.zip | awk '{print $5}')
echo -e "${GREEN}✓ Created ZIP file (size: $ZIP_SIZE)${NC}"
echo ""

# Step 4: Encode ZIP to base64
echo -e "${YELLOW}Step 4: Encoding ZIP to base64...${NC}"
BASE64_DATA=$(base64 < test-firmware.zip)
echo -e "${GREEN}✓ Encoded to base64${NC}"
echo ""

# Step 5: Send to build endpoint
echo -e "${YELLOW}Step 5: Sending build request...${NC}"
echo "Endpoint: $SERVER_URL/build"
echo "Query params:"
echo "  - owner: $OWNER"
echo "  - repo: $REPO"
echo "  - head_sha: $HEAD_SHA"
echo "  - installation_id: $INSTALLATION_ID"
echo "  - upload_url: $UPLOAD_URL"
echo ""

# Build the URL with query parameters
BUILD_URL="$SERVER_URL/build?owner=$OWNER&repo=$REPO&head_sha=$HEAD_SHA&installation_id=$INSTALLATION_ID&upload_url=$(echo $UPLOAD_URL | sed 's/\//%2F/g; s/:/%3A/g')"

# Send the request
echo "Sending request..."
RESPONSE=$(curl -s -X POST \
    -H "Content-Type: application/base64" \
    -d "$BASE64_DATA" \
    "$BUILD_URL" \
    2>&1 || echo "CURL_ERROR: $?")

# Check response
if [[ $RESPONSE == *"CURL_ERROR"* ]]; then
    echo -e "${RED}✗ Failed to send request${NC}"
    echo "Error: $RESPONSE"
    exit 1
elif [[ $RESPONSE == *"accepted"* ]]; then
    echo -e "${GREEN}✓ Build request accepted!${NC}"
    echo "Response:"
    echo "$RESPONSE" | python3 -m json.tool 2>/dev/null || echo "$RESPONSE"
elif [[ $RESPONSE == *"error"* ]]; then
    echo -e "${RED}✗ Build request failed${NC}"
    echo "Response:"
    echo "$RESPONSE" | python3 -m json.tool 2>/dev/null || echo "$RESPONSE"
else
    echo -e "${YELLOW}⚠ Unexpected response${NC}"
    echo "Response: $RESPONSE"
fi
echo ""

# Step 6: Alternative test with direct ZIP upload
echo -e "${YELLOW}Step 6: Testing with direct ZIP upload (application/zip)...${NC}"
RESPONSE_ZIP=$(curl -s -X POST \
    -H "Content-Type: application/zip" \
    --data-binary "@test-firmware.zip" \
    "$BUILD_URL" \
    2>&1 || echo "CURL_ERROR: $?")

if [[ $RESPONSE_ZIP == *"accepted"* ]]; then
    echo -e "${GREEN}✓ Direct ZIP upload accepted!${NC}"
    echo "Response:"
    echo "$RESPONSE_ZIP" | python3 -m json.tool 2>/dev/null || echo "$RESPONSE_ZIP"
elif [[ $RESPONSE_ZIP == *"error"* ]]; then
    echo -e "${RED}✗ Direct ZIP upload failed${NC}"
    echo "Response:"
    echo "$RESPONSE_ZIP" | python3 -m json.tool 2>/dev/null || echo "$RESPONSE_ZIP"
else
    echo -e "${YELLOW}⚠ Unexpected response${NC}"
    echo "Response: $RESPONSE_ZIP"
fi
echo ""

# Cleanup
echo -e "${YELLOW}Cleaning up...${NC}"
cd /
rm -rf "$TEST_DIR"
echo -e "${GREEN}✓ Cleanup complete${NC}"
echo ""

echo -e "${GREEN}Test complete!${NC}"