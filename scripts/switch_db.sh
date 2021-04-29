#!/bin/sh
psql -U postgres -c "\connect $1"