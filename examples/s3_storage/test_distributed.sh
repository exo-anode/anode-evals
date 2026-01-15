#!/bin/bash
# Distributed test script for the 3-node S3 cluster

# Kill any existing instances
pkill -f "s3_storage --node-id" 2>/dev/null

# Start all three nodes
echo "Starting 3-node cluster..."
./target/release/s3_storage --node-id 1 --port 4001 --peers "http://localhost:4002,http://localhost:4003" &
PID1=$!

./target/release/s3_storage --node-id 2 --port 4002 --peers "http://localhost:4001,http://localhost:4003" &
PID2=$!

./target/release/s3_storage --node-id 3 --port 4003 --peers "http://localhost:4001,http://localhost:4002" &
PID3=$!

# Wait for all servers to start
sleep 3

echo "Testing distributed S3 operations..."

# Test creating a bucket
echo -n "Creating bucket 'test-bucket': "
curl -X PUT http://localhost:4001/test-bucket -s -w "%{http_code}" -o /dev/null
echo ""

# Test listing buckets from different nodes
echo "Listing buckets from node 1:"
curl -X GET http://localhost:4001/ -s | grep -o '<Name>[^<]*</Name>' || echo "No buckets found"

echo "Listing buckets from node 2:"
curl -X GET http://localhost:4002/ -s | grep -o '<Name>[^<]*</Name>' || echo "No buckets found"

# Test putting an object
echo -n "Putting object 'test.txt' to node 1: "
echo "Hello, Distributed World!" | curl -X PUT http://localhost:4001/test-bucket/test.txt -d @- -s -w "%{http_code}" -o /dev/null
echo ""

# Test getting the object from different nodes
echo "Getting object from node 1:"
curl -X GET http://localhost:4001/test-bucket/test.txt -s
echo ""

echo "Getting object from node 2:"
curl -X GET http://localhost:4002/test-bucket/test.txt -s
echo ""

echo "Getting object from node 3:"
curl -X GET http://localhost:4003/test-bucket/test.txt -s
echo ""

# Test fault tolerance - kill node 3
echo -e "\nTesting fault tolerance - killing node 3..."
kill $PID3
sleep 2

# Try operations with only 2 nodes
echo -n "Creating bucket 'test-bucket-2' with 2 nodes: "
curl -X PUT http://localhost:4001/test-bucket-2 -s -w "%{http_code}" -o /dev/null
echo ""

echo -n "Putting object to 'test-bucket-2' with 2 nodes: "
echo "Still working!" | curl -X PUT http://localhost:4001/test-bucket-2/test2.txt -d @- -s -w "%{http_code}" -o /dev/null
echo ""

echo "Getting new object from node 2:"
curl -X GET http://localhost:4002/test-bucket-2/test2.txt -s
echo ""

# Cleanup
kill $PID1 $PID2 2>/dev/null
wait $PID1 $PID2 2>/dev/null

echo -e "\nDistributed test completed!"