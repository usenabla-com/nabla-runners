#!/bin/bash

# Enhanced test script with mock upload server
set -e

# Configuration
SERVER_URL="${SERVER_URL:-http://localhost:8080}"
MOCK_UPLOAD_PORT=9090

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo -e "${BLUE}     Nabla Runner Build Endpoint - Full Test Suite${NC}"
echo -e "${BLUE}═══════════════════════════════════════════════════════${NC}"
echo ""

# Step 1: Start mock upload server
echo -e "${YELLOW}Step 1: Starting mock upload server on port $MOCK_UPLOAD_PORT...${NC}"

# Create a simple Python mock server
cat > /tmp/mock_upload_server.py << 'EOF'
from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import sys

class UploadHandler(BaseHTTPRequestHandler):
    def do_POST(self):
        content_length = int(self.headers['Content-Length'])
        post_data = self.rfile.read(content_length)
        
        # Parse query parameters
        query = self.path.split('?')[1] if '?' in self.path else ''
        params = dict(param.split('=') for param in query.split('&') if '=' in param)
        
        print(f"[UPLOAD] Received artifact upload:")
        print(f"  - Owner: {params.get('owner', 'N/A')}")
        print(f"  - Repo: {params.get('repo', 'N/A')}")
        print(f"  - SHA: {params.get('head_sha', 'N/A')}")
        print(f"  - Size: {len(post_data)} bytes")
        
        # Send success response
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        response = json.dumps({"status": "uploaded", "size": len(post_data)})
        self.wfile.write(response.encode())
    
    def log_message(self, format, *args):
        # Suppress default logging
        pass

if __name__ == '__main__':
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 9090
    server = HTTPServer(('localhost', port), UploadHandler)
    print(f"Mock upload server running on port {port}")
    server.serve_forever()
EOF

python3 /tmp/mock_upload_server.py $MOCK_UPLOAD_PORT > /tmp/mock_upload.log 2>&1 &
MOCK_PID=$!
sleep 1

if kill -0 $MOCK_PID 2>/dev/null; then
    echo -e "${GREEN}✓ Mock upload server started (PID: $MOCK_PID)${NC}"
else
    echo -e "${RED}✗ Failed to start mock upload server${NC}"
    exit 1
fi
echo ""

# Cleanup function
cleanup() {
    echo -e "${YELLOW}Cleaning up...${NC}"
    kill $MOCK_PID 2>/dev/null || true
    rm -f /tmp/mock_upload_server.py
    rm -rf "$TEST_DIR"
    echo -e "${GREEN}✓ Cleanup complete${NC}"
}
trap cleanup EXIT

# Step 2: Check nabla-runner health
echo -e "${YELLOW}Step 2: Checking nabla-runner server health...${NC}"
HEALTH_RESPONSE=$(curl -s "$SERVER_URL/health" || echo "")
if [[ $HEALTH_RESPONSE == *"healthy"* ]]; then
    echo -e "${GREEN}✓ Server is healthy${NC}"
else
    echo -e "${RED}✗ Server is not responding${NC}"
    exit 1
fi
echo ""

# Step 3: Create test projects
echo -e "${YELLOW}Step 3: Creating test projects...${NC}"
TEST_DIR=$(mktemp -d)
cd "$TEST_DIR"

# Test 1: Rust/Cargo project
echo -e "${BLUE}Creating Rust project...${NC}"
mkdir -p rust-project
cat > rust-project/Cargo.toml << 'EOF'
[package]
name = "test-firmware"
version = "1.0.0"
edition = "2021"
EOF
mkdir -p rust-project/src
cat > rust-project/src/main.rs << 'EOF'
fn main() {
    println!("Rust firmware v1.0.0");
}
EOF

# Test 2: Makefile project
echo -e "${BLUE}Creating Makefile project...${NC}"
mkdir -p make-project
cat > make-project/Makefile << 'EOF'
all: firmware

firmware:
	@echo "Building firmware with Make..."
	@echo '#!/bin/sh' > firmware
	@echo 'echo "Make firmware v1.0.0"' >> firmware
	@chmod +x firmware

clean:
	rm -f firmware
EOF

echo -e "${GREEN}✓ Created test projects${NC}"
echo ""

# Function to test a project
test_project() {
    local project_name=$1
    local project_dir=$2
    
    echo -e "${BLUE}Testing $project_name...${NC}"
    
    # Create ZIP
    zip -r "${project_name}.zip" "$project_dir" > /dev/null 2>&1
    
    # Encode to base64
    BASE64_DATA=$(base64 < "${project_name}.zip")
    
    # Build URL with mock upload server (use valid 40-char SHA)
    BUILD_URL="$SERVER_URL/build?owner=test&repo=${project_name}&head_sha=abc123def456789012345678901234567890abcd&installation_id=12345&upload_url=http://localhost:${MOCK_UPLOAD_PORT}/upload"
    
    # Send request
    RESPONSE=$(curl -s -X POST \
        -H "Content-Type: application/base64" \
        -d "$BASE64_DATA" \
        "$BUILD_URL" 2>&1)
    
    if [[ $RESPONSE == *"accepted"* ]]; then
        echo -e "${GREEN}  ✓ Build successful${NC}"
        
        # Check if upload was received
        sleep 1
        if grep -q "Received artifact upload" /tmp/mock_upload.log 2>/dev/null; then
            echo -e "${GREEN}  ✓ Artifact uploaded${NC}"
        else
            echo -e "${YELLOW}  ⚠ Upload not confirmed${NC}"
        fi
    else
        echo -e "${RED}  ✗ Build failed${NC}"
        echo "  Response: $RESPONSE"
    fi
    echo ""
}

# Step 4: Test projects
echo -e "${YELLOW}Step 4: Running build tests...${NC}"
test_project "rust-project" "rust-project"
test_project "make-project" "make-project"

# Step 5: Show mock upload server logs
echo -e "${YELLOW}Step 5: Upload server activity:${NC}"
if [ -f /tmp/mock_upload.log ]; then
    cat /tmp/mock_upload.log
else
    echo "No upload logs found"
fi
echo ""

echo -e "${GREEN}═══════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}                    Test Complete!${NC}"
echo -e "${GREEN}═══════════════════════════════════════════════════════${NC}"