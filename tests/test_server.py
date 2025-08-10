#!/usr/bin/env python3
"""
Tests for the Nabla Enterprise Runner server.py
"""

import base64
import json
import os
import sys
import tempfile
import unittest
import zipfile
from io import BytesIO
from unittest.mock import patch, MagicMock

# Add the parent directory to the path so we can import server
sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

import server


class TestServerValidation(unittest.TestCase):
    """Test input validation functions."""

    def test_valid_owner_repo(self):
        """Test owner/repo validation."""
        self.assertTrue(server._is_valid_owner_repo("valid-repo"))
        self.assertTrue(server._is_valid_owner_repo("valid_repo"))
        self.assertTrue(server._is_valid_owner_repo("valid.repo"))
        self.assertTrue(server._is_valid_owner_repo("ValidRepo123"))
        
        # Invalid cases
        self.assertFalse(server._is_valid_owner_repo(""))
        self.assertFalse(server._is_valid_owner_repo("invalid/repo"))
        self.assertFalse(server._is_valid_owner_repo("invalid@repo"))
        self.assertFalse(server._is_valid_owner_repo("a" * 101))  # Too long

    def test_valid_head_sha(self):
        """Test SHA validation."""
        self.assertTrue(server._is_valid_head_sha("abc123def456"))
        self.assertTrue(server._is_valid_head_sha("1234567890abcdef"))
        self.assertTrue(server._is_valid_head_sha("a" * 40))  # Full SHA
        
        # Invalid cases
        self.assertFalse(server._is_valid_head_sha(""))
        self.assertFalse(server._is_valid_head_sha("abc123"))  # Too short
        self.assertFalse(server._is_valid_head_sha("a" * 41))  # Too long
        self.assertFalse(server._is_valid_head_sha("invalid-sha"))  # Invalid chars

    def test_validate_params(self):
        """Test parameter validation."""
        valid_args = {
            "owner": "test-owner",
            "repo": "test-repo",
            "head_sha": "abc123def456",
            "installation_id": "12345",
            "upload_url": "https://api.example.com/upload"
        }
        
        result = server._validate_params(valid_args)
        self.assertIsNotNone(result)
        self.assertEqual(result[0], "test-owner")
        self.assertEqual(result[1], "test-repo")
        self.assertEqual(result[2], "abc123def456")
        self.assertEqual(result[3], 12345)
        self.assertEqual(result[4], "https://api.example.com/upload")

        # Test invalid cases
        invalid_cases = [
            {**valid_args, "owner": ""},  # Empty owner
            {**valid_args, "repo": "invalid/repo"},  # Invalid repo
            {**valid_args, "head_sha": "short"},  # Invalid SHA
            {**valid_args, "installation_id": "0"},  # Invalid installation ID
            {**valid_args, "installation_id": "invalid"},  # Non-numeric installation ID
            {**valid_args, "upload_url": ""},  # Empty upload URL
        ]
        
        for invalid_args in invalid_cases:
            result = server._validate_params(invalid_args)
            self.assertIsNone(result, f"Should be invalid: {invalid_args}")


class TestServerEndpoints(unittest.TestCase):
    """Test server endpoints."""

    def setUp(self):
        """Set up test client."""
        server.app.config['TESTING'] = True
        self.client = server.app.test_client()

    def create_test_zip(self) -> bytes:
        """Create a test ZIP file."""
        buffer = BytesIO()
        with zipfile.ZipFile(buffer, 'w', zipfile.ZIP_DEFLATED) as zip_file:
            # Create a simple Cargo project
            zip_file.writestr('Cargo.toml', '''[package]
name = "test-firmware"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "firmware"
path = "src/main.rs"
''')
            zip_file.writestr('src/main.rs', '''fn main() {
    println!("Hello, firmware world!");
}
''')
        buffer.seek(0)
        return buffer.read()

    def test_build_endpoint_invalid_content_type(self):
        """Test build endpoint with invalid content type."""
        response = self.client.post('/build', 
                                  data=b'test data',
                                  headers={'Content-Type': 'text/html'})
        self.assertEqual(response.status_code, 415)
        data = json.loads(response.data)
        self.assertIn('unsupported media type', data['error'])

    def test_build_endpoint_missing_params(self):
        """Test build endpoint with missing parameters."""
        zip_data = self.create_test_zip()
        
        response = self.client.post('/build',
                                  data=zip_data,
                                  headers={'Content-Type': 'application/zip'})
        self.assertEqual(response.status_code, 400)
        data = json.loads(response.data)
        self.assertIn('invalid query params', data['error'])

    def test_build_endpoint_payload_too_large(self):
        """Test build endpoint with payload too large."""
        # Create a large payload (larger than MAX_UPLOAD)
        large_data = b'x' * (server.MAX_UPLOAD + 1)
        
        params = {
            'owner': 'test-owner',
            'repo': 'test-repo',
            'head_sha': 'abc123def456',
            'installation_id': '12345',
            'upload_url': 'https://api.example.com/upload'
        }
        
        response = self.client.post('/build',
                                  data=large_data,
                                  headers={'Content-Type': 'application/zip'},
                                  query_string=params)
        self.assertEqual(response.status_code, 413)
        data = json.loads(response.data)
        self.assertIn('payload too large', data['error'])

    @patch('server.subprocess.run')
    def test_build_endpoint_zip_success(self, mock_subprocess):
        """Test successful build with ZIP payload."""
        # Mock successful subprocess execution
        mock_result = MagicMock()
        mock_result.stdout = "Build completed successfully"
        mock_subprocess.return_value = mock_result
        
        zip_data = self.create_test_zip()
        
        params = {
            'owner': 'test-owner',
            'repo': 'test-repo',
            'head_sha': 'abc123def456',
            'installation_id': '12345',
            'upload_url': 'https://api.example.com/upload'
        }
        
        response = self.client.post('/build',
                                  data=zip_data,
                                  headers={'Content-Type': 'application/zip'},
                                  query_string=params)
        
        self.assertEqual(response.status_code, 202)
        data = json.loads(response.data)
        self.assertEqual(data['status'], 'accepted')
        self.assertIn('output', data)
        
        # Verify subprocess was called correctly
        mock_subprocess.assert_called_once()
        call_args = mock_subprocess.call_args
        
        # Check that the binary was called
        self.assertEqual(call_args[0][0][0], '/usr/local/bin/nabla-runner')
        
        # Check that JSON config was passed via stdin
        stdin_data = call_args[1]['input']
        config = json.loads(stdin_data)
        
        self.assertEqual(config['owner'], 'test-owner')
        self.assertEqual(config['repo'], 'test-repo')
        self.assertEqual(config['head_sha'], 'abc123def456')
        self.assertEqual(config['installation_id'], '12345')
        self.assertEqual(config['upload_url'], 'https://api.example.com/upload')
        self.assertIn('repo_data_base64', config)
        
        # Verify the base64 data can be decoded back to the original ZIP
        decoded_zip = base64.b64decode(config['repo_data_base64'])
        self.assertEqual(decoded_zip, zip_data)

    @patch('server.subprocess.run')
    def test_build_endpoint_base64_success(self, mock_subprocess):
        """Test successful build with BASE64 payload."""
        # Mock successful subprocess execution
        mock_result = MagicMock()
        mock_result.stdout = "Build completed successfully"
        mock_subprocess.return_value = mock_result
        
        zip_data = self.create_test_zip()
        base64_data = base64.b64encode(zip_data)
        
        params = {
            'owner': 'test-owner',
            'repo': 'test-repo',
            'head_sha': 'abc123def456',
            'installation_id': '12345',
            'upload_url': 'https://api.example.com/upload'
        }
        
        response = self.client.post('/build',
                                  data=base64_data,
                                  headers={'Content-Type': 'application/base64'},
                                  query_string=params)
        
        self.assertEqual(response.status_code, 202)
        data = json.loads(response.data)
        self.assertEqual(data['status'], 'accepted')
        
        # Verify subprocess was called correctly
        mock_subprocess.assert_called_once()
        call_args = mock_subprocess.call_args
        
        stdin_data = call_args[1]['input']
        config = json.loads(stdin_data)
        
        # Verify the base64 data matches what we sent
        self.assertEqual(config['repo_data_base64'], base64_data.decode('utf-8'))

    @patch('server.subprocess.run')
    def test_build_endpoint_subprocess_failure(self, mock_subprocess):
        """Test build endpoint when subprocess fails."""
        # Mock failed subprocess execution
        from subprocess import CalledProcessError
        mock_subprocess.side_effect = CalledProcessError(
            returncode=1, 
            cmd=['/usr/local/bin/nabla-runner'],
            output=None,
            stderr=None
        )
        mock_subprocess.side_effect.stdout = "Build failed with error"
        
        zip_data = self.create_test_zip()
        
        params = {
            'owner': 'test-owner',
            'repo': 'test-repo',
            'head_sha': 'abc123def456',
            'installation_id': '12345',
            'upload_url': 'https://api.example.com/upload'
        }
        
        response = self.client.post('/build',
                                  data=zip_data,
                                  headers={'Content-Type': 'application/zip'},
                                  query_string=params)
        
        self.assertEqual(response.status_code, 500)
        data = json.loads(response.data)
        self.assertEqual(data['error'], 'build failed')

    def test_build_endpoint_invalid_base64(self):
        """Test build endpoint with invalid BASE64 data."""
        invalid_base64 = b"This is not valid base64 data!!!"
        
        params = {
            'owner': 'test-owner',
            'repo': 'test-repo',
            'head_sha': 'abc123def456',
            'installation_id': '12345',
            'upload_url': 'https://api.example.com/upload'
        }
        
        response = self.client.post('/build',
                                  data=invalid_base64,
                                  headers={'Content-Type': 'application/base64'},
                                  query_string=params)
        
        # The server should still accept it and pass it to the Rust binary,
        # which will handle the base64 validation
        # But if we want to validate at the Python level, we could add that
        self.assertIn(response.status_code, [400, 500])  # Either validation error or subprocess error


class TestServerIntegration(unittest.TestCase):
    """Integration tests that test the full flow."""

    def setUp(self):
        """Set up test client."""
        server.app.config['TESTING'] = True
        self.client = server.app.test_client()

    def create_test_project_zip(self, project_type: str) -> bytes:
        """Create a test project ZIP for different build systems."""
        buffer = BytesIO()
        with zipfile.ZipFile(buffer, 'w', zipfile.ZIP_DEFLATED) as zip_file:
            if project_type == 'cargo':
                zip_file.writestr('Cargo.toml', '''[package]
name = "test-firmware"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "firmware"
path = "src/main.rs"
''')
                zip_file.writestr('src/main.rs', '''fn main() {
    println!("Hello, firmware world!");
}
''')
            elif project_type == 'makefile':
                zip_file.writestr('Makefile', '''CC=gcc
CFLAGS=-Wall -Wextra -std=c99

firmware: main.c
\t$(CC) $(CFLAGS) -o firmware main.c

clean:
\trm -f firmware

.PHONY: clean
''')
                zip_file.writestr('main.c', '''#include <stdio.h>

int main() {
    printf("Hello, firmware world!\\n");
    return 0;
}
''')
            elif project_type == 'cmake':
                zip_file.writestr('CMakeLists.txt', '''cmake_minimum_required(VERSION 3.10)
project(TestFirmware)

set(CMAKE_C_STANDARD 99)

add_executable(firmware main.c)
''')
                zip_file.writestr('main.c', '''#include <stdio.h>

int main() {
    printf("Hello, firmware world!\\n");
    return 0;
}
''')
        
        buffer.seek(0)
        return buffer.read()

    def test_content_type_normalization(self):
        """Test that both ZIP and BASE64 content types are handled correctly."""
        zip_data = self.create_test_project_zip('cargo')
        base64_data = base64.b64encode(zip_data)
        
        params = {
            'owner': 'test-owner',
            'repo': 'test-repo',
            'head_sha': 'abc123def456',
            'installation_id': '12345',
            'upload_url': 'https://httpbin.org/post'  # Test endpoint
        }
        
        with patch('server.subprocess.run') as mock_subprocess:
            mock_result = MagicMock()
            mock_result.stdout = "Test output"
            mock_subprocess.return_value = mock_result
            
            # Test ZIP upload
            response1 = self.client.post('/build',
                                       data=zip_data,
                                       headers={'Content-Type': 'application/zip'},
                                       query_string=params)
            
            # Test BASE64 upload
            response2 = self.client.post('/build',
                                       data=base64_data,
                                       headers={'Content-Type': 'application/base64'},
                                       query_string=params)
            
            # Both should succeed
            self.assertEqual(response1.status_code, 202)
            self.assertEqual(response2.status_code, 202)
            
            # Both should result in the same base64 data being passed to the Rust binary
            call1_config = json.loads(mock_subprocess.call_args_list[0][1]['input'])
            call2_config = json.loads(mock_subprocess.call_args_list[1][1]['input'])
            
            self.assertEqual(call1_config['repo_data_base64'], call2_config['repo_data_base64'])


if __name__ == '__main__':
    unittest.main()