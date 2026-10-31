from store import fetch_user as get

def sync(ids):
    return [get(user_id) for user_id in ids]
