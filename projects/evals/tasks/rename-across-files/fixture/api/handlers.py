from store import fetch_user

def show(user_id):
    return fetch_user(user_id)["id"]
