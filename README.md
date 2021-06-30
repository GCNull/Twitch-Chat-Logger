# Twitch-Chat-Logger
### This has only been tested on Linux, so it may not work on Windows.

To run the chat logger, ``cargo r --release <channel>``

This chat logger is a personal project of mine, but it does offer some good benefits.

* It uses Postgresql to store the messages. For every channel that you log a new database will be made inside of Postgres. I did this
to help keep tables organised and reduces disk usage removing the need for a ``channel`` column on the messages table.


* If the messages table is locked then the logger will see this and append messages into a txt file in the ``channels`` directory.
Once the messages table is unlocked then the logger will automatically read the txt file and insert the queued messages into the database without
any interaction from the user.


* Upon each new message, a new database connection is made. This worked well for me because if the database crashes or is offline for whatever reason
then the logger won't have to be restarted just to establish a new connection to the db.
    * *After testing this in extremely high message rate channels this proved to have very little effect on the CPU (in my case at least) however, if this is inefficient, please let me know as I'm open to any suggestion which could improve my knowledge of programming.*



After running the logger a new directory called ``channels`` will be made. This folder will be used
for any messages which have been queued up, so don't delete the folder or any files inside it. They will be automatically
deleted when needed.
