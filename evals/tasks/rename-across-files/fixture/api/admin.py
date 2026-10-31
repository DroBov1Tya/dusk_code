import store

def audit(user_id):
    return store.fetch_user(user_id)
