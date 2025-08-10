#!/bin/bash
# Build script for nabla-runner GCE image

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}Building Nabla Runner GCE Image${NC}"

# Check if packer is installed
if ! command -v packer &> /dev/null; then
    echo -e "${RED}Packer is not installed. Please install it first:${NC}"
    echo "brew install packer  # macOS"
    echo "Or download from: https://www.packer.io/downloads"
    exit 1
fi

# Check if variables file exists
if [ ! -f "variables.pkrvars.hcl" ]; then
    echo -e "${YELLOW}Creating variables file from example...${NC}"
    cp variables.pkrvars.hcl.example variables.pkrvars.hcl
    echo -e "${RED}Please edit variables.pkrvars.hcl with your GCP project ID${NC}"
    exit 1
fi

# Validate the template
echo -e "${YELLOW}Validating Packer template...${NC}"
packer validate -var-file=variables.pkrvars.hcl nabla-runner.pkr.hcl

# Build the image
echo -e "${YELLOW}Building image...${NC}"
packer build -var-file=variables.pkrvars.hcl nabla-runner.pkr.hcl

echo -e "${GREEN}Image build complete!${NC}"
echo ""
echo "You can now create instances from this image using:"
echo "  gcloud compute instances create nabla-runner-vm \\"
echo "    --image-family=nabla-runner \\"
echo "    --zone=us-central1-a \\"
echo "    --machine-type=e2-medium"