#!/usr/bin/env bash

mongo --host mongodb --port 27017 --username "$MONGO_INITDB_ROOT_USERNAME" --password "$MONGO_INITDB_ROOT_PASSWORD" <<EOF

rs.initiate({
    _id : "rs0",
    version: 1,
    members: [
        {
            "_id": 0,
            "host": "localhost:27017",
            "priority": 1
        },
    ]
}, {
    force: true
});

#rs.initiate();
rs.status();

EOF
