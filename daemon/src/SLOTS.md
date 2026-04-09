okay so the idea behind the slots is this, 

right now n slots is just hard coded as 4.

when a client sends a build request for (team, scope) the deamon checks if a build is in progress in the slot designated by fingerprint which is just user id, if the slot associated with user id is taken check the other slots, take the first free slot you find. If there are no free slots the abrasive client is going to keep polling until it gets a slot. 