This is the Deamon / server thing that runs on the hetzner box, it is like a message broker / event broker thing
right now all it does it just listen on :8400 and return 1 2 3 in successive responses to demo streaming responses. 

now it uses TLS to aithenticate the connection and then it just naively runs the cargo commands on the remote, it does
strip all "run"s and replaces them with "builds" and it does a default handling of the target platform: target platform
if unspecified is set to the same as the host  