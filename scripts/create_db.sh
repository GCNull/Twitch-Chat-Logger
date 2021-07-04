#!/bin/sh
psql -U $1 -tc "SELECT 1 FROM pg_database WHERE datname = '$2'" | grep -q 1 || psql -U $1 -c "CREATE DATABASE $2"
