#!/bin/bash
# Basic test script for the distributed S3 server

# Start a single node for basic testing
./target/release/s3_storage --node-id 1 --port 4001 --peers "http://localhost:4002,http://localhost:4003" &
PID=$!

# Wait for server to start
sleep 2

echo "Testing basic S3 operations on single node..."

# Test creating a bucket
echo -n "Creating bucket 'test-bucket': "
curl -X PUT http://localhost:4001/test-bucket -s -w "%{http_code}" -o /dev/null

echo ""

# Test listing buckets
echo "Listing buckets:"
curl -X GET http://localhost:4001/ -s | grep -o '<Name>[^<]*</Name>' || echo "No buckets found"

# Test putting an object
echo -n "Putting object 'test.txt': "
echo "Hello, World!" | curl -X PUT http://localhost:4001/test-bucket/test.txt -d @- -s -w "%{http_code}" -o /dev/null

echo ""

# Test getting an object
echo "Getting object 'test.txt':"
curl -X GET http://localhost:4001/test-bucket/test.txt -s

echo ""

# Test listing objects
echo "Listing objects in 'test-bucket':"
curl -X GET "http://localhost:4001/test-bucket?list-type=2" -s | grep -o '<Key>[^<]*</Key>' || echo "No objects found"

# Cleanup
kill $PID
wait $PID 2>/dev/null

echo "Basic test completed!"