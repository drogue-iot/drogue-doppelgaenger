db = db.getSiblingDB('twin-db')

db.createUser(
    {
        user: "twin",
        pwd: "digital",
        roles: [
            {
                role: "readWrite",
                db: "twin-db"
            }
        ]
    }
)

db.createCollection("things")
