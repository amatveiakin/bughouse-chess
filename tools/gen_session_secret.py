import string
import secrets

alphabet = string.ascii_letters + string.digits
# The recommended length is at least 512 bits.
# Letter and digits is slightly below 6 bits per character.
secret = ''.join(secrets.choice(alphabet) for i in range(128))
print(secret)
