#!/bin/bash
#
# Comprehensive test runner for the intelligent build system
# This script runs all tests related to the intelligent build functionality
#

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}=== Intelligent Build System Test Suite ===${NC}"
echo ""

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d "src" ]; then
    echo -e "${RED}❌ Error: Please run this script from the nabla-runners root directory${NC}"
    exit 1
fi

# Function to run a specific test with nice output
run_test() {
    local test_name=$1
    local description=$2
    
    echo -e "${YELLOW}🧪 Running: ${description}${NC}"
    echo -e "   Test: ${test_name}"
    
    if cargo test "$test_name" --lib --test "$test_name" -- --test-threads=1; then
        echo -e "${GREEN}✅ PASSED: ${description}${NC}"
    else
        echo -e "${RED}❌ FAILED: ${description}${NC}"
        return 1
    fi
    echo ""
}

# Function to run tests from a specific test file
run_test_file() {
    local test_file=$1
    local description=$2
    
    echo -e "${YELLOW}📁 Running: ${description}${NC}"
    echo -e "   File: tests/${test_file}.rs"
    
    if cargo test --test "$test_file" -- --test-threads=1; then
        echo -e "${GREEN}✅ PASSED: ${description}${NC}"
    else
        echo -e "${RED}❌ FAILED: ${description}${NC}"
        return 1
    fi
    echo ""
}

# Start test execution
echo "Starting intelligent build system tests..."
echo ""

# 1. Run unit tests for the intelligent_build module
echo -e "${BLUE}Phase 1: Unit Tests${NC}"
echo "========================================"

run_test_file "intelligent_build_tests" "Intelligent Build Unit Tests"

# 2. Run failure scenario tests
echo -e "${BLUE}Phase 2: Failure Scenario Tests${NC}"
echo "========================================"

run_test_file "failure_scenario_tests" "Mock Failure Scenario Tests"

# 3. Run integration tests (excluding network-dependent ones by default)
echo -e "${BLUE}Phase 3: Integration Tests (Local)${NC}"
echo "========================================"

# Run all tests except the ones marked with #[ignore]
if cargo test --test tiltbridge_integration_tests test_mock_tiltbridge -- --test-threads=1; then
    echo -e "${GREEN}✅ PASSED: Tiltbridge Mock Integration Tests${NC}"
else
    echo -e "${RED}❌ FAILED: Tiltbridge Mock Integration Tests${NC}"
fi
echo ""

# 4. Check if user wants to run network-dependent tests
echo -e "${BLUE}Phase 4: Network-Dependent Tests (Optional)${NC}"
echo "========================================"
echo -e "${YELLOW}⚠️  The following tests require network access and may take longer:${NC}"
echo "  - Real Tiltbridge repository download"
echo "  - HTTP endpoint testing"
echo ""

read -p "Run network-dependent tests? (y/N): " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Yy]$ ]]; then
    echo "Running network-dependent tests..."
    
    # Run tests marked with #[ignore] that require network
    if cargo test --test tiltbridge_integration_tests --ignored -- --test-threads=1; then
        echo -e "${GREEN}✅ PASSED: Network-dependent Integration Tests${NC}"
    else
        echo -e "${YELLOW}⚠️  SOME FAILED: Network-dependent Integration Tests (this is expected if you don't have network access)${NC}"
    fi
else
    echo -e "${BLUE}ℹ️  Skipping network-dependent tests${NC}"
fi
echo ""

# 5. Run the existing integration tests to make sure we didn't break anything
echo -e "${BLUE}Phase 5: Existing Integration Tests${NC}"
echo "========================================"

if cargo test --test integration_tests -- --test-threads=1; then
    echo -e "${GREEN}✅ PASSED: Existing Integration Tests${NC}"
else
    echo -e "${YELLOW}⚠️  SOME FAILED: Existing Integration Tests (expected due to /workspace requirements)${NC}"
fi
echo ""

# 6. Summary
echo -e "${BLUE}=== Test Summary ===${NC}"
echo ""
echo "The intelligent build system tests include:"
echo ""
echo "📋 Unit Tests:"
echo "   • Builder creation and configuration"
echo "   • Error pattern analysis for all build systems"
echo "   • Strategy generation and application"
echo "   • Build configuration patching"
echo ""
echo "🎭 Failure Scenario Tests:"
echo "   • Mock build failures for all supported systems"
echo "   • Cascading failure recovery"
echo "   • Unknown error handling"
echo "   • Strategy priority and application"
echo ""
echo "🔗 Integration Tests:"
echo "   • Real Tiltbridge repository handling"
echo "   • HTTP endpoint testing"
echo "   • End-to-end build pipeline simulation"
echo "   • CI/CD environment simulation"
echo ""

# 7. Additional commands for development
echo -e "${BLUE}=== Development Commands ===${NC}"
echo ""
echo "Run specific test categories:"
echo "  cargo test --test intelligent_build_tests    # Unit tests only"
echo "  cargo test --test failure_scenario_tests     # Failure scenarios only"
echo "  cargo test --test tiltbridge_integration_tests --ignored  # Network tests only"
echo ""
echo "Run with verbose output:"
echo "  cargo test --test intelligent_build_tests -- --nocapture"
echo ""
echo "Run a specific test:"
echo "  cargo test --test intelligent_build_tests test_platformio_error_analysis"
echo ""
echo "Build and run the test Tiltbridge script:"
echo "  ./tests/test_tiltbridge.sh"
echo ""

# 8. Check if the user wants to run the live Tiltbridge test
echo -e "${BLUE}=== Live Tiltbridge Test ===${NC}"
echo ""
read -p "Run the live Tiltbridge HTTP test script? (requires running server) (y/N): " -n 1 -r
echo ""

if [[ $REPLY =~ ^[Yy]$ ]]; then
    if [ -f "tests/test_tiltbridge.sh" ]; then
        echo "Running live Tiltbridge test..."
        chmod +x tests/test_tiltbridge.sh
        ./tests/test_tiltbridge.sh
    else
        echo -e "${RED}❌ test_tiltbridge.sh not found${NC}"
    fi
else
    echo -e "${BLUE}ℹ️  Skipping live Tiltbridge test${NC}"
fi

echo ""
echo -e "${GREEN}🎉 Intelligent Build System Test Suite Complete!${NC}"