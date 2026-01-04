#!/usr/bin/env bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}=== MapVas Release Checker ===${NC}\n"

# Get the version from Cargo.toml
CARGO_VERSION=$(grep '^version = ' Cargo.toml | head -1 | sed 's/version = "\(.*\)"/\1/')
echo -e "${GREEN}✓${NC} Current version in Cargo.toml: ${YELLOW}${CARGO_VERSION}${NC}"

# Check if CHANGELOG.md exists and has an entry for this version
echo -e "\n${YELLOW}Checking CHANGELOG.md...${NC}"
if [ ! -f "CHANGELOG.md" ]; then
    echo -e "${RED}✗${NC} CHANGELOG.md not found"
    exit 1
fi

if grep -q "## \[${CARGO_VERSION}\]" CHANGELOG.md; then
    echo -e "${GREEN}✓${NC} CHANGELOG.md has entry for version ${CARGO_VERSION}"
else
    echo -e "${RED}✗${NC} CHANGELOG.md missing entry for version ${CARGO_VERSION}"
    echo -e "  Please add a section: ## [${CARGO_VERSION}] - $(date +%Y-%m-%d)"
    exit 1
fi

# Check if version is already published on crates.io
echo -e "\n${YELLOW}Checking if version is already published...${NC}"
if cargo search mapvas | grep -q "mapvas = \"${CARGO_VERSION}\""; then
    echo -e "${RED}✗${NC} Version ${CARGO_VERSION} is already published on crates.io"
    echo -e "  Please update the version in Cargo.toml"
    exit 1
else
    echo -e "${GREEN}✓${NC} Version ${CARGO_VERSION} not yet published"
fi

# Check git status
echo -e "\n${YELLOW}Checking git status...${NC}"
if [ -n "$(git status --porcelain)" ]; then
    echo -e "${RED}✗${NC} Working directory is not clean"
    echo -e "  Please commit or stash your changes first"
    git status --short
    exit 1
else
    echo -e "${GREEN}✓${NC} Working directory is clean"
fi

# Check if current branch is main
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" != "main" ]; then
    echo -e "${YELLOW}⚠${NC}  Current branch is '${CURRENT_BRANCH}', not 'main'"
    read -p "  Continue anyway? (y/n) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        exit 1
    fi
else
    echo -e "${GREEN}✓${NC} On main branch"
fi

# Check if latest commit is pushed
echo -e "\n${YELLOW}Checking if latest commit is pushed...${NC}"
if [ -n "$(git log origin/main..HEAD)" ]; then
    echo -e "${RED}✗${NC} Local commits not pushed to origin"
    echo -e "  Please push your changes: git push"
    exit 1
else
    echo -e "${GREEN}✓${NC} All commits are pushed"
fi

# Check CI status
echo -e "\n${YELLOW}Checking CI status...${NC}"
if command -v gh &> /dev/null; then
    # Get the latest workflow run for main branch
    CI_STATUS=$(gh run list --branch main --limit 1 --json conclusion --jq '.[0].conclusion')

    if [ "$CI_STATUS" == "success" ]; then
        echo -e "${GREEN}✓${NC} CI passed on main branch"
    elif [ "$CI_STATUS" == "failure" ]; then
        echo -e "${RED}✗${NC} CI failed on main branch"
        echo -e "  Please fix CI failures before releasing"
        gh run list --branch main --limit 1
        exit 1
    elif [ "$CI_STATUS" == "null" ] || [ -z "$CI_STATUS" ]; then
        echo -e "${YELLOW}⚠${NC}  No CI run found or still in progress"
        read -p "  Continue anyway? (y/n) " -n 1 -r
        echo
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    else
        echo -e "${YELLOW}⚠${NC}  CI status: ${CI_STATUS}"
    fi
else
    echo -e "${YELLOW}⚠${NC}  GitHub CLI (gh) not installed, skipping CI check"
    echo -e "  Install with: sudo apt install gh"
fi

# Run cargo check
echo -e "\n${YELLOW}Running cargo check...${NC}"
if cargo check --quiet; then
    echo -e "${GREEN}✓${NC} cargo check passed"
else
    echo -e "${RED}✗${NC} cargo check failed"
    exit 1
fi

# Run cargo clippy
echo -e "\n${YELLOW}Running cargo clippy...${NC}"
if cargo clippy --all-targets --quiet -- -D warnings; then
    echo -e "${GREEN}✓${NC} cargo clippy passed (no warnings)"
else
    echo -e "${RED}✗${NC} cargo clippy found warnings"
    exit 1
fi

# Run tests
echo -e "\n${YELLOW}Running tests...${NC}"
if cargo test --quiet; then
    echo -e "${GREEN}✓${NC} All tests passed"
else
    echo -e "${RED}✗${NC} Tests failed"
    exit 1
fi

# Dry run cargo publish
echo -e "\n${YELLOW}Running cargo publish --dry-run...${NC}"
if cargo publish --dry-run 2>&1 | tee /tmp/cargo-publish-dry-run.log; then
    echo -e "${GREEN}✓${NC} cargo publish --dry-run succeeded"
else
    echo -e "${RED}✗${NC} cargo publish --dry-run failed"
    echo -e "  Check the output above for details"
    exit 1
fi

# Final summary
echo -e "\n${GREEN}=== All checks passed! ===${NC}"
echo -e "\nVersion ${YELLOW}${CARGO_VERSION}${NC} is ready to release."

# Prompt to create and push tag
echo -e "\n${YELLOW}Create git tag and push to GitHub?${NC}"
read -p "  This will create tag v${CARGO_VERSION} and push it. Continue? (y/n) " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    # Create the tag
    echo -e "\n${YELLOW}Creating tag v${CARGO_VERSION}...${NC}"
    if git tag -a "v${CARGO_VERSION}" -m "Release v${CARGO_VERSION}"; then
        echo -e "${GREEN}✓${NC} Tag created successfully"
    else
        echo -e "${RED}✗${NC} Failed to create tag"
        exit 1
    fi

    # Push the tag
    echo -e "\n${YELLOW}Pushing tag to GitHub...${NC}"
    if git push origin "v${CARGO_VERSION}"; then
        echo -e "${GREEN}✓${NC} Tag pushed successfully"
    else
        echo -e "${RED}✗${NC} Failed to push tag"
        echo -e "  You can manually push with: git push origin v${CARGO_VERSION}"
        exit 1
    fi

    echo -e "\n${GREEN}=== Tag created and pushed! ===${NC}"
    echo -e "\nNext steps:"
    echo -e "  1. Publish to crates:   ${YELLOW}cargo publish${NC}"
    echo -e "  2. Create GitHub release from the tag v${CARGO_VERSION}\n"
else
    echo -e "\n${YELLOW}Skipped tag creation.${NC}"
    echo -e "\nTo create and push the tag manually:"
    echo -e "  ${YELLOW}git tag -a v${CARGO_VERSION} -m 'Release v${CARGO_VERSION}'${NC}"
    echo -e "  ${YELLOW}git push origin v${CARGO_VERSION}${NC}\n"
fi
